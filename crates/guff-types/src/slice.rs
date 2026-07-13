//! Port of `cmd/compile/internal/types2/slice.go`.

use crate::arena::{TypeArena, TypeData, TypeId};

/// A slice type.
///
/// Equivalent to `types2.Slice`.
#[derive(Debug, Clone)]
pub struct Slice {
    elem: TypeId,
}

impl Slice {
    pub fn elem(&self) -> TypeId {
        self.elem
    }
}

/// Construct a new slice type.
///
/// Equivalent to `types2.NewSlice`.
pub fn new_slice(arena: &mut TypeArena, elem: TypeId) -> TypeId {
    arena.alloc(TypeData::Slice(Slice { elem }))
}

/// Free-function accessor — panics if `id` is not a Slice.
pub fn slice_elem(arena: &TypeArena, id: TypeId) -> TypeId {
    match arena.get(id) {
        TypeData::Slice(s) => s.elem,
        other => panic!("expected Slice, got {:?}", std::mem::discriminant(other)),
    }
}
