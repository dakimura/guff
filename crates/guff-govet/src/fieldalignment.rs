//! `fieldalignment` — structs that would use less memory if their fields were
//! sorted.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/fieldalignment`.
//!
//! One of the ten analyzers `cmd/vet` leaves off and golangci-lint only runs
//! under `govet.enable-all` or an explicit `enable`. fiber turns `enable-all`
//! on, and four of its `//nolint:govet` directives are there for this check —
//! with the analyzer missing, guff reported all four as unused directives.
//!
//! The size model here is the analyzer's own `gcSizes`, **not** `types.Sizes`:
//! upstream carries a private copy because it needs `ptrdata` as well, and the
//! two differ in what they answer for a zero-sized trailing field. Only the
//! word size and max alignment come from the pass's `TypesSizes`.

use std::sync::OnceLock;

use guff::ast::{Expr, Field, FieldList, StructType};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit};
use guff_types::arena::{ObjectArena, TypeArena, TypeData};
use guff_types::basic::BasicKind;
use guff_types::{array_elem, array_len, struct_field, struct_num_fields, TypeId};

/// `gcSizes` — the analyzer's private size model.
struct GcSizes {
    word_size: i64,
    max_align: i64,
}

/// `align(x, a)`: the smallest `y >= x` with `y % a == 0`.
fn align(x: i64, a: i64) -> i64 {
    let y = x + a - 1;
    y - y % a
}

/// `basicSizes`, by kind. Absent kinds (`int`, `uint`, `uintptr`, `string`,
/// `unsafe.Pointer`, the untyped ones) fall through to the caller's handling.
fn basic_size(kind: BasicKind) -> Option<i64> {
    Some(match kind {
        BasicKind::Bool | BasicKind::Int8 | BasicKind::Uint8 => 1,
        BasicKind::Int16 | BasicKind::Uint16 => 2,
        BasicKind::Int32 | BasicKind::Uint32 | BasicKind::Float32 => 4,
        BasicKind::Int64 | BasicKind::Uint64 | BasicKind::Float64 | BasicKind::Complex64 => 8,
        BasicKind::Complex128 => 16,
        _ => return None,
    })
}

impl GcSizes {
    fn alignof(&self, arena: &TypeArena, objs: &ObjectArena, t: TypeId) -> i64 {
        let u = t.underlying(arena);
        match arena.get(u) {
            // "For a variable x of array type: unsafe.Alignof(x) is the same
            // as unsafe.Alignof(x[0]), but at least 1."
            TypeData::Array(_) => return self.alignof(arena, objs, array_elem(arena, u)),
            // "…the largest of the values unsafe.Alignof(x.f) for each field
            // f of x, but at least 1."
            TypeData::Struct(_) => {
                let mut max = 1;
                for i in 0..struct_num_fields(arena, u) {
                    let ft = field_type(arena, objs, u, i);
                    let a = self.alignof(arena, objs, ft);
                    if a > max {
                        max = a;
                    }
                }
                return max;
            }
            _ => {}
        }
        let a = self.sizeof(arena, objs, t);
        if a < 1 {
            return 1;
        }
        if a > self.max_align {
            return self.max_align;
        }
        a
    }

    fn sizeof(&self, arena: &TypeArena, objs: &ObjectArena, t: TypeId) -> i64 {
        let u = t.underlying(arena);
        match arena.get(u) {
            TypeData::Basic(b) => {
                let kind = b.kind();
                if let Some(sz) = basic_size(kind) {
                    return sz;
                }
                if kind == BasicKind::String {
                    return self.word_size * 2;
                }
            }
            TypeData::Array(_) => {
                let elem = array_elem(arena, u);
                return array_len(arena, u) * self.sizeof(arena, objs, elem);
            }
            TypeData::Slice(_) => return self.word_size * 3,
            TypeData::Struct(_) => {
                let nf = struct_num_fields(arena, u);
                if nf == 0 {
                    return 0;
                }
                let mut o = 0i64;
                let mut max = 1i64;
                for i in 0..nf {
                    let ft = field_type(arena, objs, u, i);
                    let a = self.alignof(arena, objs, ft);
                    let mut sz = self.sizeof(arena, objs, ft);
                    if a > max {
                        max = a;
                    }
                    // A zero-sized final field of a non-empty struct is padded
                    // to one byte, so that a pointer past it stays inside the
                    // allocation.
                    if i == nf - 1 && sz == 0 && o != 0 {
                        sz = 1;
                    }
                    o = align(o, a) + sz;
                }
                return align(o, max);
            }
            TypeData::Interface(_) => return self.word_size * 2,
            _ => {}
        }
        // Catch-all: pointer, map, chan, signature, `unsafe.Pointer`, and the
        // word-sized basics (`int`, `uint`, `uintptr`).
        self.word_size
    }

    /// How many leading bytes the garbage collector must scan for pointers.
    ///
    /// This is the second of the analyzer's two diagnostics: a struct can be
    /// optimally *sized* and still make the collector scan further than it
    /// needs to.
    fn ptrdata(&self, arena: &TypeArena, objs: &ObjectArena, t: TypeId) -> i64 {
        let u = t.underlying(arena);
        match arena.get(u) {
            TypeData::Basic(b) => match b.kind() {
                BasicKind::String | BasicKind::UnsafePointer => self.word_size,
                _ => 0,
            },
            TypeData::Chan(_)
            | TypeData::Map(_)
            | TypeData::Pointer(_)
            | TypeData::Signature(_)
            | TypeData::Slice(_) => self.word_size,
            TypeData::Interface(_) => 2 * self.word_size,
            TypeData::Array(_) => {
                let n = array_len(arena, u);
                if n == 0 {
                    return 0;
                }
                let elem = array_elem(arena, u);
                let a = self.ptrdata(arena, objs, elem);
                if a == 0 {
                    return 0;
                }
                let z = self.sizeof(arena, objs, elem);
                (n - 1) * z + a
            }
            TypeData::Struct(_) => {
                let nf = struct_num_fields(arena, u);
                if nf == 0 {
                    return 0;
                }
                let mut o = 0i64;
                let mut p = 0i64;
                for i in 0..nf {
                    let ft = field_type(arena, objs, u, i);
                    let a = self.alignof(arena, objs, ft);
                    let sz = self.sizeof(arena, objs, ft);
                    let fp = self.ptrdata(arena, objs, ft);
                    o = align(o, a);
                    if fp != 0 {
                        p = o + fp;
                    }
                    o += sz;
                }
                p
            }
            // Upstream panics here ("impossible"); a type parameter or an
            // ill-typed package can reach it in guff, where a panic would take
            // the whole run down.
            _ => 0,
        }
    }
}

/// A field's declared type. `struct_field` hands back the field object; its
/// type lives in the object arena.
fn field_type(arena: &TypeArena, objs: &ObjectArena, struct_ty: TypeId, i: usize) -> TypeId {
    struct_field(arena, struct_ty, i).typ(objs).unwrap_or_else(|| {
        guff_types::basic::lookup_basic(arena, BasicKind::Invalid)
            .expect("universe defines BasicKind::Invalid")
    })
}

/// One entry of `optimalOrder`'s sort.
struct Elem {
    index: usize,
    alignof: i64,
    sizeof: i64,
    ptrdata: i64,
}

/// `optimalOrder`: the permutation of the fields that the gc layout packs
/// tightest, and the sizes it achieves.
fn optimal_order(arena: &TypeArena, objs: &ObjectArena, s: &GcSizes, struct_ty: TypeId) -> Vec<Elem> {
    let nf = struct_num_fields(arena, struct_ty);
    let mut elems: Vec<Elem> = (0..nf)
        .map(|i| {
            let ft = field_type(arena, objs, struct_ty, i);
            Elem {
                index: i,
                alignof: s.alignof(arena, objs, ft),
                sizeof: s.sizeof(arena, objs, ft),
                ptrdata: s.ptrdata(arena, objs, ft),
            }
        })
        .collect();

    // `sort.Slice` is not stable, but every comparison below is decided by the
    // field's measurements alone and ties are left in place, so a stable sort
    // gives the same answer for the shapes the diagnostic depends on.
    elems.sort_by(|ei, ej| {
        use std::cmp::Ordering;
        // Zero-sized objects first.
        let zeroi = ei.sizeof == 0;
        let zeroj = ej.sizeof == 0;
        if zeroi != zeroj {
            return if zeroi { Ordering::Less } else { Ordering::Greater };
        }
        // Then more tightly aligned objects before less tightly aligned ones.
        if ei.alignof != ej.alignof {
            return ej.alignof.cmp(&ei.alignof);
        }
        // Pointerful objects before pointer-free ones.
        let noptrsi = ei.ptrdata == 0;
        let noptrsj = ej.ptrdata == 0;
        if noptrsi != noptrsj {
            return if noptrsj { Ordering::Less } else { Ordering::Greater };
        }
        if !noptrsi {
            // Both have pointers: the one with fewer trailing non-pointer
            // bytes goes first, so the field with the most trailing
            // non-pointer bytes ends the pointerful section.
            let traili = ei.sizeof - ei.ptrdata;
            let trailj = ej.sizeof - ej.ptrdata;
            if traili != trailj {
                return traili.cmp(&trailj);
            }
        }
        // Lastly by size, largest first.
        if ei.sizeof != ej.sizeof {
            return ej.sizeof.cmp(&ei.sizeof);
        }
        Ordering::Equal
    });
    elems
}

/// The size and pointer-bytes of a struct laid out in the order `elems` gives,
/// without building a `types.Struct` for it.
///
/// Upstream calls `s.Sizeof(types.NewStruct(reordered))`; the arena has no
/// cheap way to synthesize a struct type, and the two loops below are the same
/// arithmetic `Sizeof`/`ptrdata` do over a struct's fields.
fn layout(s: &GcSizes, elems: &[Elem]) -> (i64, i64) {
    if elems.is_empty() {
        return (0, 0);
    }
    let mut o = 0i64;
    let mut max = 1i64;
    let n = elems.len();
    for (i, e) in elems.iter().enumerate() {
        let mut sz = e.sizeof;
        if e.alignof > max {
            max = e.alignof;
        }
        if i == n - 1 && sz == 0 && o != 0 {
            sz = 1;
        }
        o = align(o, e.alignof) + sz;
    }
    let size = align(o, max);

    let mut o = 0i64;
    let mut p = 0i64;
    for e in elems {
        o = align(o, e.alignof);
        if e.ptrdata != 0 {
            p = o + e.ptrdata;
        }
        o += e.sizeof;
    }
    (size, p)
}

/// The reordered struct, rendered the way upstream renders it: comments and
/// doc dropped, multi-name fields flattened to one name each, and printed
/// against a fresh `FileSet` so the printer lays it out canonically rather
/// than reproducing the original's line breaks.
fn render_reordered(node: &StructType, elems: &[Elem]) -> Option<String> {
    let mut flat: Vec<Field> = Vec::new();
    for f in &node.fields.list {
        if f.names.len() <= 1 {
            let mut f = f.clone();
            f.doc = None;
            f.comment = None;
            flat.push(f);
            continue;
        }
        for name in &f.names {
            flat.push(Field {
                doc: None,
                names: vec![name.clone()],
                ty: f.ty.clone(),
                tag: f.tag.clone(),
                comment: None,
                id: 0,
            });
        }
    }
    let mut reordered = Vec::with_capacity(elems.len());
    for e in elems {
        reordered.push(flat.get(e.index)?.clone());
    }
    let new_struct = StructType {
        struct_: guff::position::Pos(0),
        fields: FieldList {
            opening: guff::position::Pos(0),
            list: reordered,
            closing: guff::position::Pos(0),
        },
        incomplete: false,
        id: 0,
    };
    let expr = Expr::StructType(new_struct);
    let fset = guff::position::FileSet::new();
    let mut buf: Vec<u8> = Vec::new();
    guff::format::node(&mut buf, &fset, guff::printer::PrintNode::Expr(&expr)).ok()?;
    String::from_utf8(buf).ok()
}

fn check_struct(pass: &Pass<'_>, node: &StructType) -> Option<Diagnostic> {
    let info = pass.types_info()?;
    let typ = info.types.get(&node.id)?.typ;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let arena = &artifacts.types;
    let objs = &artifacts.objects;
    if !matches!(arena.get(typ.underlying(arena)), TypeData::Struct(_)) {
        return None;
    }
    let struct_ty = typ.underlying(arena);

    // `pass.TypesSizes.Sizeof(unsafe.Pointer)` / `.Alignof(unsafe.Pointer)`.
    let sizes = guff_types::default_sizes();
    let s = GcSizes {
        word_size: sizes.word_size,
        max_align: sizes.max_align,
    };

    let elems = optimal_order(arena, objs, &s, struct_ty);
    let (optsz, optptrs) = layout(&s, &elems);

    let sz = s.sizeof(arena, objs, struct_ty);
    let message = if sz != optsz {
        format!("struct of size {sz} could be {optsz}")
    } else {
        let ptrs = s.ptrdata(arena, objs, struct_ty);
        if ptrs != optptrs {
            format!("struct with {ptrs} pointer bytes could be {optptrs}")
        } else {
            // Already optimal.
            return None;
        }
    };

    let pos = node.struct_.0 as u32;
    let mut diag = Diagnostic {
        pos,
        // `End: node.Pos() + len("struct")` — the keyword, not the body.
        end: pos + "struct".len() as u32,
        message,
        ..Diagnostic::default()
    };
    if let Some(text) = render_reordered(node, &elems) {
        diag.suggested_fixes = vec![SuggestedFix {
            message: "Rearrange fields".to_string(),
            text_edits: vec![TextEdit {
                pos,
                end: node.fields.end().0 as u32,
                new_text: text,
            }],
        }];
    }
    Some(diag)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "fieldalignment requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(StructType), pass.files(), |n| {
        let NodeRef::StructType(s) = n else {
            return;
        };
        if let Some(diag) = check_struct(pass, s) {
            pending.push(diag);
        }
    });

    for diag in pending {
        pass.report(diag);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "fieldalignment",
        doc: "find structs that would use less memory if their fields were sorted",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/fieldalignment",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
