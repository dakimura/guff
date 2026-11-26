//! Port of `cmd/compile/internal/types2/pointer.go`.

use serde::{Deserialize, Serialize};

use crate::arena::{TypeArena, TypeData, TypeId};

/// A pointer type.
///
/// Equivalent to `types2.Pointer`. The field is named `base` (matching Go) to
/// emphasise that this is the pointed-to type — the accessor is [`Pointer::elem`]
/// to match `types2.Pointer.Elem`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pointer {
    base: TypeId,
}

impl Pointer {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.base = r.ty(self.base);
    }
}

impl Pointer {
    pub fn elem(&self) -> TypeId {
        self.base
    }
}

/// Construct a new pointer type whose element (base) type is `elem`.
///
/// Equivalent to `types2.NewPointer`.
pub fn new_pointer(arena: &mut TypeArena, elem: TypeId) -> TypeId {
    arena.alloc(TypeData::Pointer(Pointer { base: elem }))
}

/// Free-function accessor — panics if `id`'s underlying type is not a Pointer.
///
/// Defined types whose underlying type is a pointer are accepted: we call
/// [`TypeId::underlying`] first, matching [`crate::slice::slice_elem`].
pub fn pointer_elem(arena: &TypeArena, id: TypeId) -> TypeId {
    let id = id.underlying(arena);
    match arena.get(id) {
        TypeData::Pointer(p) => p.base,
        other => panic!("expected Pointer, got {:?}", std::mem::discriminant(other)),
    }
}
