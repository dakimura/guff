//! Port of `cmd/compile/internal/types2/sizes.go` (plus `gcsizes.go` and the
//! `gccgosizes.go` arch table).
//!
//! Implements the `Sizes` machinery that backs package `unsafe`'s
//! `Sizeof`/`Alignof`/`Offsetof`. Decoupled from `Checker` — these are pure
//! structural computations over the type/object/package arenas.
//!
//! ## One Rust type for two Go types
//!
//! Go has two concrete `Sizes` implementations:
//!
//! - `StdSizes` (the documented public convenience type), used by `gccgo`.
//! - `gcSizes` (`gcsizes.go`), used by the `gc` compiler — this is what the
//!   default `stdSizes = SizesFor("gc", "amd64")` actually is.
//!
//! They share `Alignof` and `Offsetsof` verbatim; only `Sizeof` differs in two
//! places (the array-size formula and the struct trailing-padding rules). To
//! avoid duplicating ~120 lines we model both with a single [`Sizes`] struct
//! carrying a [`SizesKind`] discriminant; the kind only changes the `Array` and
//! `Struct` branches of [`Sizes::sizeof`].
//!
//! A negative size/offset means "type too large" (overflow), matching Go.
//!
//! ## Deferred
//!
//! `Config.sizes`/`Config.alignof`/`offsetof` wiring (the `Checker` side that
//! picks `conf.Sizes` or the default) is left to the chunk that ports the
//! `unsafe.*` builtins (D23) — that needs package imports first (D16). This
//! module supplies the algorithm; the driver will call it.

use crate::arena::{ObjectArena, ObjectId, PackageArena, TypeArena, TypeData, TypeId};
use crate::array::{array_elem, array_len};
use crate::basic::{basic_info, basic_kind, BasicKind, IS_STRING};
use crate::lookup::as_named;
use crate::named::named_obj;
use crate::predicates::{is_complex, is_type_param, is_typed};
use crate::r#struct::{struct_field, struct_num_fields};

/// Which Go `Sizes` implementation a [`Sizes`] value mimics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizesKind {
    /// `StdSizes` (`sizes.go`) — used by `gccgo`.
    Std,
    /// `gcSizes` (`gcsizes.go`) — used by the `gc` compiler; the default.
    Gc,
}

/// Sizing functions for package `unsafe`. Mirrors Go's `StdSizes`/`gcSizes`.
///
/// `word_size` must be >= 4 (32 bits); `max_align` must be >= 1.
#[derive(Debug, Clone, Copy)]
pub struct Sizes {
    pub kind: SizesKind,
    pub word_size: i64,
    pub max_align: i64,
}

impl Sizes {
    /// `StdSizes{WordSize, MaxAlign}` (the `gccgo` flavour).
    pub fn std(word_size: i64, max_align: i64) -> Self {
        Sizes {
            kind: SizesKind::Std,
            word_size,
            max_align,
        }
    }

    /// `gcSizes{WordSize, MaxAlign}` (the `gc` flavour).
    pub fn gc(word_size: i64, max_align: i64) -> Self {
        Sizes {
            kind: SizesKind::Gc,
            word_size,
            max_align,
        }
    }

    /// `(*StdSizes).Alignof` / `(*gcSizes).Alignof` — identical between the two.
    ///
    /// The result is always >= 1.
    pub fn alignof(&self, ta: &TypeArena, oa: &ObjectArena, pa: &PackageArena, t: TypeId) -> i64 {
        let result = self.alignof_inner(ta, oa, pa, t);
        debug_assert!(result >= 1);
        result
    }

    fn alignof_inner(&self, ta: &TypeArena, oa: &ObjectArena, pa: &PackageArena, t: TypeId) -> i64 {
        // For arrays and structs, alignment is defined in terms of alignment of
        // the elements and fields, respectively.
        let u = t.underlying(ta);
        match ta.get(u) {
            // spec: "For a variable x of array type: unsafe.Alignof(x) is the
            // same as unsafe.Alignof(x[0]), but at least 1."
            TypeData::Array(_) => {
                let elem = array_elem(ta, u);
                return self.alignof(ta, oa, pa, elem);
            }
            TypeData::Struct(_) => {
                let n = struct_num_fields(ta, u);
                if n == 0 && is_sync_atomic_align64(ta, oa, pa, t) {
                    // Special case: sync/atomic.align64 is an empty struct we
                    // recognize as a signal that the struct it contains must be
                    // 64-bit-aligned.
                    return 8;
                }
                // spec: "For a variable x of struct type: unsafe.Alignof(x) is
                // the largest of the values unsafe.Alignof(x.f) for each field f
                // of x, but at least 1."
                let mut max = 1i64;
                for i in 0..n {
                    let ftyp = field_typ(ta, oa, u, i);
                    let a = self.alignof(ta, oa, pa, ftyp);
                    if a > max {
                        max = a;
                    }
                }
                return max;
            }
            // Multiword data structures are effectively structs in which each
            // element has size WordSize. Type parameters lead to variable
            // sizes/alignments; Alignof won't be called for them.
            TypeData::Slice(_) | TypeData::Interface(_) => {
                debug_assert!(!is_type_param(ta, t));
                return self.word_size;
            }
            // Strings are like slices and interfaces.
            TypeData::Basic(_) => {
                if basic_info(ta, u).contains(IS_STRING) {
                    return self.word_size;
                }
            }
            TypeData::TypeParam(_) | TypeData::Union(_) => panic!("unreachable"),
            _ => {}
        }

        let mut a = self.sizeof(ta, oa, pa, t); // may be 0 or negative
                                                // spec: "For a variable x of any type: unsafe.Alignof(x) is at least 1."
        if a < 1 {
            return 1;
        }
        // complex{64,128} are aligned like [2]float{32,64}.
        if is_complex(ta, t) {
            a /= 2;
        }
        if a > self.max_align {
            return self.max_align;
        }
        a
    }

    /// `(*StdSizes).Offsetsof` / `(*gcSizes).Offsetsof` — identical between the
    /// two. `fields` are the struct's field objects, in declaration order.
    ///
    /// A negative entry indicates the struct is too large at that point.
    pub fn offsetsof(
        &self,
        ta: &TypeArena,
        oa: &ObjectArena,
        pa: &PackageArena,
        fields: &[ObjectId],
    ) -> Vec<i64> {
        let mut offsets = vec![0i64; fields.len()];
        let mut offs = 0i64;
        for (i, &f) in fields.iter().enumerate() {
            if offs < 0 {
                // all remaining offsets are too large
                offsets[i] = -1;
                continue;
            }
            // offs >= 0
            let ftyp = f.typ(oa).expect("struct field has a type");
            let a = self.alignof(ta, oa, pa, ftyp);
            offs = align(offs, a); // possibly < 0 if align overflows
            offsets[i] = offs;
            let d = self.sizeof(ta, oa, pa, ftyp);
            if d >= 0 && offs >= 0 {
                offs += d; // ok to overflow to < 0
            } else {
                offs = -1; // f.typ or offs is too large
            }
        }
        offsets
    }

    /// `(*StdSizes).Sizeof` / `(*gcSizes).Sizeof`.
    ///
    /// A negative result indicates that `t` is too large.
    pub fn sizeof(&self, ta: &TypeArena, oa: &ObjectArena, pa: &PackageArena, t: TypeId) -> i64 {
        let u = t.underlying(ta);
        match ta.get(u) {
            TypeData::Basic(_) => {
                debug_assert!(is_typed(ta, t));
                let k = basic_kind(ta, u);
                let s = basic_size(k);
                if s > 0 {
                    return s;
                }
                if k == BasicKind::String {
                    return self.word_size * 2;
                }
            }
            TypeData::Array(_) => {
                let n = array_len(ta, u);
                if n <= 0 {
                    return 0;
                }
                // n > 0
                let elem = array_elem(ta, u);
                let esize = self.sizeof(ta, oa, pa, elem);
                if esize < 0 {
                    return -1; // element too large
                }
                if esize == 0 {
                    return 0; // 0-size element
                }
                // esize > 0
                match self.kind {
                    SizesKind::Std => {
                        let a = self.alignof(ta, oa, pa, elem);
                        let ea = align(esize, a); // possibly < 0 if align overflows
                        if ea < 0 {
                            return -1;
                        }
                        // ea >= 1
                        let n1 = n - 1; // n1 >= 0
                                        // Final size is ea*n1 + esize; size must be <= maxInt64.
                        if n1 > 0 && ea > i64::MAX / n1 {
                            return -1; // ea*n1 overflows
                        }
                        return ea.wrapping_mul(n1).wrapping_add(esize); // may overflow to < 0, ok
                    }
                    SizesKind::Gc => {
                        // Final size is esize * n; size must be <= maxInt64.
                        if esize > i64::MAX / n {
                            return -1; // esize * n overflows
                        }
                        return esize * n;
                    }
                }
            }
            TypeData::Slice(_) => return self.word_size * 3,
            TypeData::Struct(_) => {
                let n = struct_num_fields(ta, u);
                if n == 0 {
                    return 0;
                }
                let fields = struct_field_ids(ta, u);
                let offsets = self.offsetsof(ta, oa, pa, &fields);
                let offs = offsets[n - 1];
                let last_typ = field_typ(ta, oa, u, n - 1);
                let mut size = self.sizeof(ta, oa, pa, last_typ);
                if offs < 0 || size < 0 {
                    return -1; // type too large
                }
                match self.kind {
                    SizesKind::Std => return offs + size, // may overflow to < 0, ok
                    SizesKind::Gc => {
                        // gc: The last field of a non-zero-sized struct is not
                        // allowed to have size 0.
                        if offs > 0 && size == 0 {
                            size = 1;
                        }
                        // gc: Size includes alignment padding.
                        let a = self.alignof(ta, oa, pa, u);
                        return align(offs + size, a); // may overflow to < 0, ok
                    }
                }
            }
            TypeData::Interface(_) => {
                // Type parameters lead to variable sizes/alignments; Sizeof
                // won't be called for them.
                debug_assert!(!is_type_param(ta, t));
                return self.word_size * 2;
            }
            TypeData::TypeParam(_) | TypeData::Union(_) => panic!("unreachable"),
            _ => {}
        }
        self.word_size // catch-all
    }
}

/// `basicSizes` table from `sizes.go` — the byte size of explicitly sized
/// basic types. Returns 0 for kinds not in the table (Int/Uint/Uintptr/String/
/// pointers/untyped), which fall through to the word-size catch-all.
fn basic_size(kind: BasicKind) -> i64 {
    use BasicKind::*;
    match kind {
        Bool | Int8 | Uint8 => 1,
        Int16 | Uint16 => 2,
        Int32 | Uint32 | Float32 => 4,
        Int64 | Uint64 | Float64 | Complex64 => 8,
        Complex128 => 16,
        _ => 0,
    }
}

/// `IsSyncAtomicAlign64` — recognises the empty `sync/atomic.align64` (or
/// `internal/runtime/atomic.align64`) marker struct.
pub fn is_sync_atomic_align64(
    ta: &TypeArena,
    oa: &ObjectArena,
    pa: &PackageArena,
    t: TypeId,
) -> bool {
    let named = match as_named(ta, t) {
        Some(n) => n,
        None => return false,
    };
    let obj = named_obj(ta, named);
    if obj.name(oa) != "align64" {
        return false;
    }
    match obj.pkg(oa) {
        Some(pkg) => {
            let path = pa.get(pkg).path();
            path == "sync/atomic" || path == "internal/runtime/atomic"
        }
        None => false,
    }
}

/// `align` returns the smallest `y >= x` such that `y % a == 0`. `a` must be a
/// power of 2 within 1..=8. The result may be negative due to overflow.
pub fn align(x: i64, a: i64) -> i64 {
    debug_assert!(x >= 0 && (1..=8).contains(&a) && a & (a - 1) == 0);
    (x.wrapping_add(a - 1)) & !(a - 1)
}

/// Common gc architecture word sizes / alignments (`gcArchSizes`).
fn gc_arch_sizes(arch: &str) -> Option<(i64, i64)> {
    Some(match arch {
        "386" => (4, 4),
        "amd64" => (8, 8),
        "amd64p32" => (4, 8),
        "arm" => (4, 4),
        "arm64" => (8, 8),
        "loong64" => (8, 8),
        "mips" => (4, 4),
        "mipsle" => (4, 4),
        "mips64" => (8, 8),
        "mips64le" => (8, 8),
        "ppc64" => (8, 8),
        "ppc64le" => (8, 8),
        "riscv64" => (8, 8),
        "s390x" => (8, 8),
        "sparc64" => (8, 8),
        "wasm" => (8, 8),
        _ => return None,
    })
}

/// gccgo architecture word sizes / alignments (`gccgoArchSizes`).
fn gccgo_arch_sizes(arch: &str) -> Option<(i64, i64)> {
    Some(match arch {
        "386" => (4, 4),
        "alpha" => (8, 8),
        "amd64" => (8, 8),
        "amd64p32" => (4, 8),
        "arm" => (4, 8),
        "armbe" => (4, 8),
        "arm64" => (8, 8),
        "arm64be" => (8, 8),
        "ia64" => (8, 8),
        "loong64" => (8, 8),
        "m68k" => (4, 2),
        "mips" => (4, 8),
        "mipsle" => (4, 8),
        "mips64" => (8, 8),
        "mips64le" => (8, 8),
        "mips64p32" => (4, 8),
        "mips64p32le" => (4, 8),
        "nios2" => (4, 8),
        "ppc" => (4, 8),
        "ppc64" => (8, 8),
        "ppc64le" => (8, 8),
        "riscv" => (4, 8),
        "riscv64" => (8, 8),
        "s390" => (4, 8),
        "s390x" => (8, 8),
        "sh" => (4, 8),
        "shbe" => (4, 8),
        "sparc" => (4, 8),
        "sparc64" => (8, 8),
        "wasm" => (8, 8),
        _ => return None,
    })
}

/// `SizesFor` — the `Sizes` used by a compiler for an architecture. Returns
/// `None` if the compiler/architecture pair is unknown.
///
/// Supported `compiler` values: `"gc"` and `"gccgo"`.
pub fn sizes_for(compiler: &str, arch: &str) -> Option<Sizes> {
    match compiler {
        "gc" => gc_arch_sizes(arch).map(|(w, a)| Sizes::gc(w, a)),
        "gccgo" => gccgo_arch_sizes(arch).map(|(w, a)| Sizes::std(w, a)),
        _ => None,
    }
}

/// `stdSizes` — the default `Sizes` used when `Config.Sizes == nil`
/// (`SizesFor("gc", "amd64")`, i.e. `gcSizes{8, 8}`).
pub fn default_sizes() -> Sizes {
    sizes_for("gc", "amd64").expect("gc/amd64 is a known arch")
}

// --- small internal helpers ------------------------------------------------

/// Type of the `i`'th field of struct `u`.
fn field_typ(ta: &TypeArena, oa: &ObjectArena, u: TypeId, i: usize) -> TypeId {
    let f = struct_field(ta, u, i);
    f.typ(oa).expect("struct field has a type")
}

/// All field object ids of struct `u`, in declaration order.
fn struct_field_ids(ta: &TypeArena, u: TypeId) -> Vec<ObjectId> {
    let n = struct_num_fields(ta, u);
    (0..n).map(|i| struct_field(ta, u, i)).collect()
}
