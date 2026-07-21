//! Port of the `Const` parts of `cmd/compile/internal/types2/object.go`.
//!
//! Module is named `const_` because `const` is a Rust keyword.

use guff_constant::Value;

use crate::arena::{ObjectArena, ObjectData, ObjectId, TypeId};
use crate::object::{HasMeta, ObjectMeta};

/// A declared constant — an `Object` with both a type and a compile-time
/// value.
///
/// Equivalent to `types2.Const`.
#[derive(Debug, Clone)]
pub struct Const {
    name: String,
    typ: TypeId,
    val: Value,
    pub(crate) meta: ObjectMeta,
}

impl Const {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.typ = r.ty(self.typ);
        self.meta.remap_ids(r);
    }
}

impl Const {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn typ(&self) -> TypeId {
        self.typ
    }

    /// The constant's compile-time value.
    pub fn val(&self) -> &Value {
        &self.val
    }

    /// Set the constant's type (filled in during `constDecl`, replacing the
    /// resolver's `Typ[Invalid]` placeholder).
    pub fn set_typ(&mut self, typ: TypeId) {
        self.typ = typ;
    }

    /// Set the constant's compile-time value.
    pub fn set_val(&mut self, val: Value) {
        self.val = val;
    }
}

impl HasMeta for Const {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// Construct a new [`Const`].
///
/// Equivalent to `types2.NewConst`.
pub fn new_const(
    arena: &mut ObjectArena,
    name: impl Into<String>,
    typ: TypeId,
    val: Value,
) -> ObjectId {
    arena.alloc(ObjectData::Const(Const {
        name: name.into(),
        typ,
        val,
        meta: ObjectMeta::default(),
    }))
}
