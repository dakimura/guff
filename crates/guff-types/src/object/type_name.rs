//! Stub port of the `TypeName` parts of `cmd/compile/internal/types2/object.go`.
//!
//! Like [`crate::object::func::Func`], a `TypeName`'s `typ` is two-phase:
//! `None` immediately after `new_type_name(.., None)`, then populated when
//! the Named/Alias/TypeParam that references it back-fills the binding.
//! `pkg`, `pos`, `parent` scope land alongside Scope/Package.

use crate::arena::{ObjectArena, ObjectData, ObjectId, TypeId};
use crate::object::{HasMeta, ObjectMeta};

/// A type name — the [`Object`-like](crate::ObjectId) entity bound by a
/// `type T = ...`, `type T struct{...}`, or generic type-parameter
/// declaration. Each `TypeName` references the type it binds to via [`typ`].
///
/// Equivalent to `types2.TypeName`. The `IsAlias` predicate (true iff the
/// bound type is an `Alias`) is straightforward to add once needed — it just
/// asks the arena for the variant.
#[derive(Debug, Clone)]
pub struct TypeName {
    name: String,
    typ: Option<TypeId>,
    pub(crate) meta: ObjectMeta,
}

impl TypeName {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn typ(&self) -> Option<TypeId> {
        self.typ
    }

    /// Set the bound type. Used during two-phase construction when the
    /// referenced Named/Alias/TypeParam is allocated *after* the TypeName.
    pub fn set_typ(&mut self, typ: TypeId) {
        self.typ = Some(typ);
    }
}

impl HasMeta for TypeName {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// Construct a new [`TypeName`].
///
/// Equivalent to `types2.NewTypeName`. Pass `None` for `typ` when the
/// referenced type isn't built yet (Go's typical pattern: allocate TypeName
/// → allocate Named pointing to it → set TypeName.typ to the Named).
pub fn new_type_name(
    arena: &mut ObjectArena,
    name: impl Into<String>,
    typ: Option<TypeId>,
) -> ObjectId {
    arena.alloc(ObjectData::TypeName(TypeName {
        name: name.into(),
        typ,
        meta: ObjectMeta::default(),
    }))
}

/// Mutating accessor — sets the bound type on an existing TypeName.
///
/// # Panics
/// Panics if `id` does not refer to a `TypeName`.
pub fn type_name_set_typ(arena: &mut ObjectArena, id: ObjectId, typ: TypeId) {
    match arena.get_mut(id) {
        ObjectData::TypeName(tn) => tn.set_typ(typ),
        other => panic!("expected TypeName, got {:?}", std::mem::discriminant(other)),
    }
}
