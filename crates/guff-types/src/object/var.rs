//! Stub port of the `Var` parts of `cmd/compile/internal/types2/object.go`.
//!
//! Only the fields needed by `Tuple` in chunk 1 — `name` and `typ`. The full
//! `Var` (with Parent scope, Package, Pos, embedded-field flag, `IsField`,
//! `IsParam`, `Origin`, etc.) is filled in alongside Scope/Package.

use crate::arena::{ObjectArena, ObjectData, ObjectId, TypeId};
use crate::object::{HasMeta, ObjectMeta};

/// Discriminator for what kind of variable an object represents.
///
/// Equivalent to `types2.VarKind`. Discriminant `0` is reserved as
/// "unset" to match Go's `_ VarKind = iota` placeholder.
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub enum VarKind {
    #[default]
    Unset = 0,
    /// Package-level variable.
    Package = 1,
    /// Local variable.
    Local = 2,
    /// Method receiver variable.
    Recv = 3,
    /// Function parameter variable.
    Param = 4,
    /// Function result variable.
    Result = 5,
    /// Struct field.
    Field = 6,
}

/// A variable, struct field, function parameter, or function result.
///
/// Equivalent to `types2.Var`.
#[derive(Debug, Clone)]
pub struct Var {
    name: String,
    typ: TypeId,
    kind: VarKind,
    embedded: bool,
    pub(crate) meta: ObjectMeta,
}

impl Var {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn typ(&self) -> TypeId {
        self.typ
    }

    /// Set the variable's type (filled in during `varDecl`, replacing the
    /// resolver's `Typ[Invalid]` placeholder).
    pub fn set_typ(&mut self, typ: TypeId) {
        self.typ = typ;
    }

    pub fn kind(&self) -> VarKind {
        self.kind
    }

    /// Set the kind. Should be called immediately after construction (Go's
    /// API requires it for non-`PackageVar` cases).
    pub fn set_kind(&mut self, kind: VarKind) {
        self.kind = kind;
    }

    /// `true` for an embedded struct field (Go: `NewField(.., embedded=true)`).
    pub fn embedded(&self) -> bool {
        self.embedded
    }

    /// Reports whether this Var is a struct field.
    pub fn is_field(&self) -> bool {
        self.kind == VarKind::Field
    }

    /// Reports whether this Var is a function parameter.
    pub fn is_param(&self) -> bool {
        self.kind == VarKind::Param
    }

    /// Same as [`Var::embedded`] — provided for parity with Go's
    /// `Var.Anonymous` legacy method.
    pub fn anonymous(&self) -> bool {
        self.embedded
    }
}

impl HasMeta for Var {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// Construct a new [`Var`] with the given name and type. Kind defaults to
/// [`VarKind::Package`] (matches Go's `NewVar`). Use [`new_param`] /
/// [`new_field`] for the other common cases, or call [`Var::set_kind`]
/// after construction.
pub fn new_var(arena: &mut ObjectArena, name: impl Into<String>, typ: TypeId) -> ObjectId {
    arena.alloc(ObjectData::Var(Var {
        name: name.into(),
        typ,
        kind: VarKind::Package,
        embedded: false,
        meta: ObjectMeta::default(),
    }))
}

/// Construct a new [`Var`] representing a function parameter. Kind
/// defaults to [`VarKind::Param`]; the caller can reassign via
/// [`Var::set_kind`] for receivers / results.
///
/// Equivalent to `types2.NewParam`.
pub fn new_param(arena: &mut ObjectArena, name: impl Into<String>, typ: TypeId) -> ObjectId {
    arena.alloc(ObjectData::Var(Var {
        name: name.into(),
        typ,
        kind: VarKind::Param,
        embedded: false,
        meta: ObjectMeta::default(),
    }))
}

/// Construct a new [`Var`] representing a struct field. For embedded
/// fields, `name` is the unqualified type name under which the field is
/// accessible.
///
/// Equivalent to `types2.NewField`.
pub fn new_field(
    arena: &mut ObjectArena,
    name: impl Into<String>,
    typ: TypeId,
    embedded: bool,
) -> ObjectId {
    arena.alloc(ObjectData::Var(Var {
        name: name.into(),
        typ,
        kind: VarKind::Field,
        embedded,
        meta: ObjectMeta::default(),
    }))
}
