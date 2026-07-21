//! Port of `cmd/compile/internal/types2/array.go`.

use crate::arena::{TypeArena, TypeData, TypeId};

/// An array type.
///
/// Equivalent to `types2.Array`. A negative `len` indicates an unknown length
/// (matches Go's convention for partially-resolved types).
#[derive(Debug, Clone)]
pub struct Array {
    len: i64,
    elem: TypeId,
}

impl Array {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.elem = r.ty(self.elem);
    }
}

impl Array {
    pub fn len(&self) -> i64 {
        self.len
    }

    pub fn elem(&self) -> TypeId {
        self.elem
    }
}

/// Construct a new array type. A negative `len` signals "unknown length".
///
/// Equivalent to `types2.NewArray`.
pub fn new_array(arena: &mut TypeArena, elem: TypeId, len: i64) -> TypeId {
    arena.alloc(TypeData::Array(Array { len, elem }))
}

/// Free-function accessor — panics if `id` is not an Array.
pub fn array_len(arena: &TypeArena, id: TypeId) -> i64 {
    as_array(arena, id).len
}

pub fn array_elem(arena: &TypeArena, id: TypeId) -> TypeId {
    as_array(arena, id).elem
}

fn as_array(arena: &TypeArena, id: TypeId) -> &Array {
    match arena.get(id) {
        TypeData::Array(a) => a,
        other => panic!("expected Array, got {:?}", std::mem::discriminant(other)),
    }
}
