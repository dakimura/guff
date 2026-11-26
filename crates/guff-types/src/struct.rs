//! Port of `cmd/compile/internal/types2/struct.go`.
//!
//! Module name is `r#struct` because `struct` is a Rust keyword; users should
//! import via the re-export at the crate root or refer to it as
//! `guff_types::r#struct`.
//!
//! Chunk 2 ports only the data-and-accessor portion of `struct.go` — the
//! `objset`-based duplicate-field-name check from `NewStruct` is deferred
//! until the full `Object` infrastructure (with `Id()` / scope membership)
//! lands. Callers are expected to ensure field-name uniqueness themselves
//! until then.

use serde::{Deserialize, Serialize};

use crate::arena::{ObjectId, TypeArena, TypeData, TypeId};

/// A struct type.
///
/// Equivalent to `types2.Struct`. Fields are stored as [`ObjectId`]s pointing
/// to `Var` objects in the [`crate::arena::ObjectArena`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Struct {
    fields: Vec<ObjectId>,
    /// Field tags; may be shorter than `fields` (or empty) if trailing fields
    /// have no tag.
    tags: Vec<String>,
}

impl Struct {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        for f in &mut self.fields {
            *f = r.obj(*f);
        }
    }
}

impl Struct {
    /// Number of fields, including blank and embedded fields.
    pub fn num_fields(&self) -> usize {
        self.fields.len()
    }

    /// The `i`'th field; panics if `i >= num_fields()`.
    pub fn field(&self, i: usize) -> ObjectId {
        self.fields[i]
    }

    /// The `i`'th field tag, or `""` if the field has no tag.
    pub fn tag(&self, i: usize) -> &str {
        if i < self.tags.len() {
            &self.tags[i]
        } else {
            ""
        }
    }
}

/// Construct a new struct type.
///
/// Equivalent to `types2.NewStruct`. If a field with index `i` has a tag,
/// `tags[i]` must be that tag; `tags` may be shorter than `fields` (only as
/// long as needed to hold the largest-indexed tag). Pass an empty `tags` if
/// no field has a tag.
///
/// # Panics
/// Panics if `tags.len() > fields.len()`.
///
/// # Caveat
/// Unlike Go's `NewStruct`, this does **not** check for duplicate field names
/// (the check requires the full `objset` machinery which arrives in a later
/// chunk). Callers are responsible for ensuring uniqueness until then.
pub fn new_struct(arena: &mut TypeArena, fields: Vec<ObjectId>, tags: Vec<String>) -> TypeId {
    if tags.len() > fields.len() {
        panic!("more tags than fields");
    }
    arena.alloc(TypeData::Struct(Struct { fields, tags }))
}

/// Free-function accessor — panics if `id` is not a Struct.
pub fn struct_num_fields(arena: &TypeArena, id: TypeId) -> usize {
    as_struct(arena, id).num_fields()
}

pub fn struct_field(arena: &TypeArena, id: TypeId, i: usize) -> ObjectId {
    as_struct(arena, id).field(i)
}

pub fn struct_tag<'a>(arena: &'a TypeArena, id: TypeId, i: usize) -> &'a str {
    as_struct(arena, id).tag(i)
}

fn as_struct(arena: &TypeArena, id: TypeId) -> &Struct {
    match arena.get(id) {
        TypeData::Struct(s) => s,
        other => panic!("expected Struct, got {:?}", std::mem::discriminant(other)),
    }
}
