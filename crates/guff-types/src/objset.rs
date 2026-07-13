//! Port of `cmd/compile/internal/types2/objset.go`.
//!
//! An `ObjSet` is a HashMap keyed by `Object.id()` — the
//! package-path-qualified name for unexported identifiers. This is similar
//! to a [`crate::scope::Scope`] but keyed by `id`, not by raw name (so
//! that unexported identifiers from different packages don't collide).

use std::collections::HashMap;

use crate::arena::{ObjectArena, ObjectId, PackageArena};

/// A set of objects identified by their unique `id` (per [`crate::object::id`]).
///
/// Equivalent to `types2.objset`.
#[derive(Debug, Default, Clone)]
pub struct ObjSet {
    elems: HashMap<String, ObjectId>,
}

impl ObjSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.elems.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elems.is_empty()
    }

    /// Attempts to insert `obj` keyed by its `id`. If a different object
    /// with the same id is already present, returns that alternative and
    /// leaves the set unchanged. Otherwise inserts and returns `None`.
    ///
    /// Equivalent to `types2.objset.insert`.
    pub fn insert(
        &mut self,
        oarena: &ObjectArena,
        parena: &PackageArena,
        obj: ObjectId,
    ) -> Option<ObjectId> {
        let id = obj.id(oarena, parena);
        if let Some(&alt) = self.elems.get(&id) {
            return Some(alt);
        }
        self.elems.insert(id, obj);
        None
    }

    /// Look up an entry by its `id` string.
    pub fn get(&self, id: &str) -> Option<ObjectId> {
        self.elems.get(id).copied()
    }
}
