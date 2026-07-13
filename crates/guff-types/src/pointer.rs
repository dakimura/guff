//! Port of `cmd/compile/internal/types2/pointer.go`.

use crate::arena::{TypeArena, TypeData, TypeId};

/// A pointer type.
///
/// Equivalent to `types2.Pointer`. The field is named `base` (matching Go) to
/// emphasise that this is the pointed-to type — the accessor is [`Pointer::elem`]
/// to match `types2.Pointer.Elem`.
#[derive(Debug, Clone)]
pub struct Pointer {
    base: TypeId,
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

/// Free-function accessor — panics if `id` is not a Pointer.
pub fn pointer_elem(arena: &TypeArena, id: TypeId) -> TypeId {
    match arena.get(id) {
        TypeData::Pointer(p) => p.base,
        other => panic!("expected Pointer, got {:?}", std::mem::discriminant(other)),
    }
}
