//! Port of `cmd/compile/internal/types2/basic.go`.
//!
//! The predeclared-types initializer ([`init_universe`]) is a partial port of
//! the `Typ` / `basicAliases` tables from `universe.go` — only the `*Basic`
//! entries. Predeclared functions, constants, `nil`, `any`, `error`,
//! `comparable` will follow in a later chunk when their backing object kinds
//! exist.

use crate::arena::{TypeArena, TypeData, TypeId};

/// Kind of a basic type.
///
/// Numeric discriminants are stable and match `types2.BasicKind`. `Byte` and
/// `Rune` are not enum variants because they're aliases for `Uint8` and
/// `Int32` (same numeric value); see the [`BYTE`] and [`RUNE`] constants.
///
/// Equivalent to `types2.BasicKind`.
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum BasicKind {
    Invalid = 0,

    // Predeclared types.
    Bool = 1,
    Int = 2,
    Int8 = 3,
    Int16 = 4,
    Int32 = 5,
    Int64 = 6,
    Uint = 7,
    Uint8 = 8,
    Uint16 = 9,
    Uint32 = 10,
    Uint64 = 11,
    Uintptr = 12,
    Float32 = 13,
    Float64 = 14,
    Complex64 = 15,
    Complex128 = 16,
    String = 17,
    UnsafePointer = 18,

    // Types for untyped values.
    UntypedBool = 19,
    UntypedInt = 20,
    UntypedRune = 21,
    UntypedFloat = 22,
    UntypedComplex = 23,
    UntypedString = 24,
    UntypedNil = 25,
}

/// Alias: `byte` is `uint8`.
pub const BYTE: BasicKind = BasicKind::Uint8;
/// Alias: `rune` is `int32`.
pub const RUNE: BasicKind = BasicKind::Int32;

/// Number of valid [`BasicKind`] discriminants, including `Invalid`. Used to
/// size the predeclared-types lookup table.
pub const BASIC_KIND_COUNT: usize = 26;

/// Bitset of properties of a basic type. Stored as a plain `u32` rather than
/// via `bitflags`, matching Go's `BasicInfo int`.
///
/// Equivalent to `types2.BasicInfo`.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct BasicInfo(pub u32);

impl BasicInfo {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, other: BasicInfo) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn union(self, other: BasicInfo) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for BasicInfo {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

// Property flags (single-bit values).
pub const IS_BOOLEAN: BasicInfo = BasicInfo(1 << 0);
pub const IS_INTEGER: BasicInfo = BasicInfo(1 << 1);
pub const IS_UNSIGNED: BasicInfo = BasicInfo(1 << 2);
pub const IS_FLOAT: BasicInfo = BasicInfo(1 << 3);
pub const IS_COMPLEX: BasicInfo = BasicInfo(1 << 4);
pub const IS_STRING: BasicInfo = BasicInfo(1 << 5);
pub const IS_UNTYPED: BasicInfo = BasicInfo(1 << 6);

// Composite flags (`is_ordered`, `is_numeric`, `is_const_type`). Computed at
// compile time so they're cheap to use in `BasicInfo::contains` checks.
pub const IS_ORDERED: BasicInfo = BasicInfo(IS_INTEGER.0 | IS_FLOAT.0 | IS_STRING.0);
pub const IS_NUMERIC: BasicInfo = BasicInfo(IS_INTEGER.0 | IS_FLOAT.0 | IS_COMPLEX.0);
pub const IS_CONST_TYPE: BasicInfo = BasicInfo(IS_BOOLEAN.0 | IS_NUMERIC.0 | IS_STRING.0);

/// A basic Go type — `int`, `string`, `bool`, `untyped int`, etc.
///
/// Equivalent to `types2.Basic`.
#[derive(Debug, Clone)]
pub struct Basic {
    kind: BasicKind,
    info: BasicInfo,
    name: String,
}

impl Basic {
    pub fn kind(&self) -> BasicKind {
        self.kind
    }

    pub fn info(&self) -> BasicInfo {
        self.info
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Free-function accessors mirroring the `(b *Basic) Kind/Info/Name` Go API,
/// usable when only a [`TypeId`] is in hand.
///
/// # Panics
/// Panics if `id` does not refer to a `Basic`.
pub fn basic_kind(arena: &TypeArena, id: TypeId) -> BasicKind {
    as_basic(arena, id).kind
}

pub fn basic_info(arena: &TypeArena, id: TypeId) -> BasicInfo {
    as_basic(arena, id).info
}

pub fn basic_name<'a>(arena: &'a TypeArena, id: TypeId) -> &'a str {
    &as_basic(arena, id).name
}

fn as_basic(arena: &TypeArena, id: TypeId) -> &Basic {
    match arena.get(id) {
        TypeData::Basic(b) => b,
        other => panic!("expected Basic, got {:?}", std::mem::discriminant(other)),
    }
}

/// Find the predeclared `Basic` type of the given `kind` in `arena`, mirroring
/// Go's `Typ[kind]` lookup for callers that hold only a [`TypeArena`] (e.g. the
/// SSA builder, which is handed the checker's arena but not its `Typ` table).
///
/// Basic types are singletons registered once by [`init_universe`], so at most
/// one entry matches. Returns `None` if the arena has no such basic type (an
/// arena that was not seeded from the universe).
pub fn lookup_basic(arena: &TypeArena, kind: BasicKind) -> Option<TypeId> {
    for i in 1..=arena.len() {
        let id = TypeId::from_index(i);
        if let TypeData::Basic(b) = arena.get(id) {
            if b.kind == kind {
                return Some(id);
            }
        }
    }
    None
}

/// Initialize the predeclared basic types into a fresh arena.
///
/// Returns a populated [`TypeArena`] and a lookup table indexed by
/// `BasicKind as usize` — `table[BasicKind::Int as usize]` gives the
/// [`TypeId`] for predeclared `int`.
///
/// The `byte`/`rune` aliases are **not** in the table (they reuse `Uint8` /
/// `Int32` entries); use the [`BYTE`] / [`RUNE`] constants directly when you
/// need them.
///
/// Equivalent to the `Typ = [...]*Basic{...}` table in `universe.go`.
pub fn init_universe() -> (TypeArena, [TypeId; BASIC_KIND_COUNT]) {
    let mut arena = TypeArena::new();
    // Allocate the predeclared Basic types in BasicKind discriminant order so
    // table indexing matches Go's `Typ[BasicKind]`.
    let mut table: [Option<TypeId>; BASIC_KIND_COUNT] = [None; BASIC_KIND_COUNT];

    let mut define = |arena: &mut TypeArena, kind: BasicKind, info: BasicInfo, name: &str| {
        let id = arena.alloc(TypeData::Basic(Basic {
            kind,
            info,
            name: name.to_string(),
        }));
        table[kind as usize] = Some(id);
    };

    define(
        &mut arena,
        BasicKind::Invalid,
        BasicInfo::empty(),
        "invalid type",
    );

    define(&mut arena, BasicKind::Bool, IS_BOOLEAN, "bool");
    define(&mut arena, BasicKind::Int, IS_INTEGER, "int");
    define(&mut arena, BasicKind::Int8, IS_INTEGER, "int8");
    define(&mut arena, BasicKind::Int16, IS_INTEGER, "int16");
    define(&mut arena, BasicKind::Int32, IS_INTEGER, "int32");
    define(&mut arena, BasicKind::Int64, IS_INTEGER, "int64");
    define(
        &mut arena,
        BasicKind::Uint,
        IS_INTEGER | IS_UNSIGNED,
        "uint",
    );
    define(
        &mut arena,
        BasicKind::Uint8,
        IS_INTEGER | IS_UNSIGNED,
        "uint8",
    );
    define(
        &mut arena,
        BasicKind::Uint16,
        IS_INTEGER | IS_UNSIGNED,
        "uint16",
    );
    define(
        &mut arena,
        BasicKind::Uint32,
        IS_INTEGER | IS_UNSIGNED,
        "uint32",
    );
    define(
        &mut arena,
        BasicKind::Uint64,
        IS_INTEGER | IS_UNSIGNED,
        "uint64",
    );
    define(
        &mut arena,
        BasicKind::Uintptr,
        IS_INTEGER | IS_UNSIGNED,
        "uintptr",
    );
    define(&mut arena, BasicKind::Float32, IS_FLOAT, "float32");
    define(&mut arena, BasicKind::Float64, IS_FLOAT, "float64");
    define(&mut arena, BasicKind::Complex64, IS_COMPLEX, "complex64");
    define(&mut arena, BasicKind::Complex128, IS_COMPLEX, "complex128");
    define(&mut arena, BasicKind::String, IS_STRING, "string");
    // Go calls the predeclared name "Pointer" (capital P) because it lives in
    // the `unsafe` package; here we mirror that exactly.
    define(
        &mut arena,
        BasicKind::UnsafePointer,
        BasicInfo::empty(),
        "Pointer",
    );

    define(
        &mut arena,
        BasicKind::UntypedBool,
        IS_BOOLEAN | IS_UNTYPED,
        "untyped bool",
    );
    define(
        &mut arena,
        BasicKind::UntypedInt,
        IS_INTEGER | IS_UNTYPED,
        "untyped int",
    );
    define(
        &mut arena,
        BasicKind::UntypedRune,
        IS_INTEGER | IS_UNTYPED,
        "untyped rune",
    );
    define(
        &mut arena,
        BasicKind::UntypedFloat,
        IS_FLOAT | IS_UNTYPED,
        "untyped float",
    );
    define(
        &mut arena,
        BasicKind::UntypedComplex,
        IS_COMPLEX | IS_UNTYPED,
        "untyped complex",
    );
    define(
        &mut arena,
        BasicKind::UntypedString,
        IS_STRING | IS_UNTYPED,
        "untyped string",
    );
    define(&mut arena, BasicKind::UntypedNil, IS_UNTYPED, "untyped nil");

    let unwrapped = table.map(|opt| opt.expect("every BasicKind variant must be populated"));
    (arena, unwrapped)
}
