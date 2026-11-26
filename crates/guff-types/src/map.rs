//! Port of `cmd/compile/internal/types2/map.go`.

use serde::{Deserialize, Serialize};

use crate::arena::{TypeArena, TypeData, TypeId};

/// A map type.
///
/// Equivalent to `types2.Map`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Map {
    key: TypeId,
    elem: TypeId,
}

impl Map {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.key = r.ty(self.key);
        self.elem = r.ty(self.elem);
    }
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

/// Free-function accessor — returns key/elem for a Map, or the Invalid basic
/// type when hybrid source-checking left an incomplete type.
pub fn map_key(arena: &TypeArena, id: TypeId) -> TypeId {
    match arena.get(id) {
        TypeData::Map(m) => m.key,
        _ => crate::basic::lookup_basic(arena, crate::basic::BasicKind::Invalid)
            .expect("universe must define BasicKind::Invalid"),
    }
}

pub fn map_elem(arena: &TypeArena, id: TypeId) -> TypeId {
    match arena.get(id) {
        TypeData::Map(m) => m.elem,
        _ => crate::basic::lookup_basic(arena, crate::basic::BasicKind::Invalid)
            .expect("universe must define BasicKind::Invalid"),
    }
}
