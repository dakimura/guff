//! Port of the `Builtin` parts of `cmd/compile/internal/types2/object.go`
//! plus the `builtinId` enum and `predeclaredFuncs` table from `universe.go`.
//!
//! Builtins don't have a valid type — their `typ` always references
//! `Typ[Invalid]`. The Checker handles each builtin specially at call sites
//! based on its [`BuiltinId`].

use serde::{Deserialize, Serialize};

use crate::arena::{ObjectArena, ObjectData, ObjectId, TypeId};
use crate::object::{HasMeta, ObjectMeta};

/// Identifier for a built-in function.
///
/// Equivalent to `types2.builtinId`. Numeric discriminants match Go's
/// `iota` ordering — keep variants in the same order if more are added.
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum BuiltinId {
    // Universe scope.
    Append = 0,
    Cap,
    Clear,
    Close,
    Complex,
    Copy,
    Delete,
    Imag,
    Len,
    Make,
    Max,
    Min,
    New,
    Panic,
    Print,
    Println,
    Real,
    Recover,

    // Package unsafe.
    Add,
    Alignof,
    Offsetof,
    Sizeof,
    Slice,
    SliceData,
    String,
    StringData,

    // Testing support — only registered by `DefPredeclaredTestFuncs`.
    Assert,
    Trace,
}

/// The syntactic role an expression plays, as classified by `rawExpr`.
/// Mirrors types2's `exprKind` (`conversion`/`expression`/`statement`).
///
/// For a builtin's static metadata only `Expression`/`Statement` are used
/// (Go's `predeclaredFuncs[...].kind`); `Conversion` arises dynamically from
/// `callExpr` when the callee turns out to be a type.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ExprKind {
    /// A type conversion `T(x)`.
    Conversion,
    /// An ordinary value expression (also the builtin-usable-as-expression
    /// case, e.g. `len`, `cap`).
    Expression,
    /// A form allowed in statement position: a function/method call, a
    /// receive, or a statement-only builtin (`close`, `panic`, …).
    Statement,
}

/// Static signature info for a built-in: name, expected argument count,
/// variadic flag, expression-vs-statement.
///
/// Equivalent to the entries in `types2.predeclaredFuncs`.
#[derive(Copy, Clone, Debug)]
pub struct BuiltinInfo {
    pub name: &'static str,
    pub nargs: u8,
    pub variadic: bool,
    pub kind: ExprKind,
}

/// Lookup-by-id table for builtin metadata. Indexed by `BuiltinId as usize`.
///
/// Equivalent to `types2.predeclaredFuncs`.
pub const PREDECLARED_FUNCS: [BuiltinInfo; 28] = [
    // Universe scope (matches `BuiltinId` discriminant order).
    BuiltinInfo {
        name: "append",
        nargs: 1,
        variadic: true,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "cap",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "clear",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Statement,
    },
    BuiltinInfo {
        name: "close",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Statement,
    },
    BuiltinInfo {
        name: "complex",
        nargs: 2,
        variadic: false,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "copy",
        nargs: 2,
        variadic: false,
        kind: ExprKind::Statement,
    },
    BuiltinInfo {
        name: "delete",
        nargs: 2,
        variadic: false,
        kind: ExprKind::Statement,
    },
    BuiltinInfo {
        name: "imag",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "len",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "make",
        nargs: 1,
        variadic: true,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "max",
        nargs: 1,
        variadic: true,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "min",
        nargs: 1,
        variadic: true,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "new",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "panic",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Statement,
    },
    BuiltinInfo {
        name: "print",
        nargs: 0,
        variadic: true,
        kind: ExprKind::Statement,
    },
    BuiltinInfo {
        name: "println",
        nargs: 0,
        variadic: true,
        kind: ExprKind::Statement,
    },
    BuiltinInfo {
        name: "real",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "recover",
        nargs: 0,
        variadic: false,
        kind: ExprKind::Statement,
    },
    // Package unsafe.
    BuiltinInfo {
        name: "Add",
        nargs: 2,
        variadic: false,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "Alignof",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "Offsetof",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "Sizeof",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "Slice",
        nargs: 2,
        variadic: false,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "SliceData",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "String",
        nargs: 2,
        variadic: false,
        kind: ExprKind::Expression,
    },
    BuiltinInfo {
        name: "StringData",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Expression,
    },
    // Testing support.
    BuiltinInfo {
        name: "assert",
        nargs: 1,
        variadic: false,
        kind: ExprKind::Statement,
    },
    BuiltinInfo {
        name: "trace",
        nargs: 0,
        variadic: true,
        kind: ExprKind::Statement,
    },
];

/// Returns the [`BuiltinInfo`] for `id`.
pub fn builtin_info(id: BuiltinId) -> &'static BuiltinInfo {
    &PREDECLARED_FUNCS[id as usize]
}

/// A predeclared / unsafe-package built-in function.
///
/// Equivalent to `types2.Builtin`. The `typ` field always references
/// `Typ[Invalid]` because built-ins don't have a normal Go function type
/// (they're called specially by the type checker).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Builtin {
    name: String,
    typ: TypeId, // always Typ[Invalid]
    id: BuiltinId,
    pub(crate) meta: ObjectMeta,
}

impl Builtin {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn typ(&self) -> TypeId {
        self.typ
    }

    pub fn id(&self) -> BuiltinId {
        self.id
    }
}

impl HasMeta for Builtin {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// Construct a new [`Builtin`].
///
/// Equivalent to `types2.newBuiltin`. `invalid_typ` must be the predeclared
/// `Typ[Invalid]` from the universe initialisation.
pub fn new_builtin(arena: &mut ObjectArena, id: BuiltinId, invalid_typ: TypeId) -> ObjectId {
    let info = builtin_info(id);
    arena.alloc(ObjectData::Builtin(Builtin {
        name: info.name.to_string(),
        typ: invalid_typ,
        id,
        meta: ObjectMeta::default(),
    }))
}
