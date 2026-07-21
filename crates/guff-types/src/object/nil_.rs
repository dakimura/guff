//! Port of the `Nil` parts of `cmd/compile/internal/types2/object.go`.
//!
//! Module is named `nil_` because `nil` reads as a built-in concept in Rust
//! (though not a keyword) — keeping the trailing underscore avoids surprise.
//!
//! Go has exactly one `Nil` value (the predeclared `nil`), and its type is
//! always the predeclared `UntypedNil` basic type.

use crate::arena::{ObjectArena, ObjectData, ObjectId, TypeId};
use crate::object::{HasMeta, ObjectMeta};

/// The predeclared `nil`.
///
/// Equivalent to `types2.Nil`.
#[derive(Debug, Clone)]
pub struct Nil {
    name: String, // always "nil"
    typ: TypeId,  // always Typ[UntypedNil]
    pub(crate) meta: ObjectMeta,
}

impl Nil {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.typ = r.ty(self.typ);
        self.meta.remap_ids(r);
    }
}

impl Nil {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn typ(&self) -> TypeId {
        self.typ
    }
}

impl HasMeta for Nil {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// Construct the predeclared `nil` object. Should be called exactly once
/// per `Universe` initialisation.
pub fn new_nil(arena: &mut ObjectArena, untyped_nil_typ: TypeId) -> ObjectId {
    arena.alloc(ObjectData::Nil(Nil {
        name: "nil".to_string(),
        typ: untyped_nil_typ,
        meta: ObjectMeta::default(),
    }))
}
