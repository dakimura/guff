//! Port of `cmd/compile/internal/types2/slice.go`.

use serde::{Deserialize, Serialize};

use crate::arena::{TypeArena, TypeData, TypeId};

/// A slice type.
///
/// Equivalent to `types2.Slice`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slice {
    elem: TypeId,
}

impl Slice {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.elem = r.ty(self.elem);
    }
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

/// Free-function accessor — panics if `id`'s underlying type is not a Slice.
///
/// Named / Alias slice types (e.g. `type Bytes []byte`) are resolved via
/// [`TypeId::underlying`] first, matching [`crate::signature::signature_params`].
pub fn slice_elem(arena: &TypeArena, id: TypeId) -> TypeId {
    let id = id.underlying(arena);
    match arena.get(id) {
        TypeData::Slice(s) => s.elem,
        other => panic!("expected Slice, got {:?}", std::mem::discriminant(other)),
    }
}
