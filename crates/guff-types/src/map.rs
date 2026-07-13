//! Port of `cmd/compile/internal/types2/map.go`.

use crate::arena::{TypeArena, TypeData, TypeId};

/// A map type.
///
/// Equivalent to `types2.Map`.
#[derive(Debug, Clone)]
pub struct Map {
    key: TypeId,
    elem: TypeId,
}

impl Map {
    pub fn key(&self) -> TypeId {
        self.key
    }

    pub fn elem(&self) -> TypeId {
        self.elem
    }
}

/// Construct a new map type.
///
/// Equivalent to `types2.NewMap`.
pub fn new_map(arena: &mut TypeArena, key: TypeId, elem: TypeId) -> TypeId {
    arena.alloc(TypeData::Map(Map { key, elem }))
}

/// Free-function accessor — panics if `id` is not a Map.
pub fn map_key(arena: &TypeArena, id: TypeId) -> TypeId {
    as_map(arena, id).key
}

pub fn map_elem(arena: &TypeArena, id: TypeId) -> TypeId {
    as_map(arena, id).elem
}

fn as_map(arena: &TypeArena, id: TypeId) -> &Map {
    match arena.get(id) {
        TypeData::Map(m) => m,
        other => panic!("expected Map, got {:?}", std::mem::discriminant(other)),
    }
}
