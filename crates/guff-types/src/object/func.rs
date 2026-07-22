//! Stub port of the `Func` parts of `cmd/compile/internal/types2/object.go`.
//!
//! Holds only the fields needed by chunk 2 — name and typ (a [`TypeId`]
//! pointing to a `Signature`). `pkg`, `pos`, `parent` scope, `origin`,
//! `hasPtrRecv_`, and the FullName/Scope/Pkg helpers land with Scope/Package.

use serde::{Deserialize, Serialize};

use crate::arena::{ObjectArena, ObjectData, ObjectId, TypeArena, TypeData, TypeId};
use crate::object::{HasMeta, ObjectMeta};

/// A declared function or method.
///
/// Equivalent to `types2.Func`. The `typ` should point to a `Signature`; we
/// don't enforce that statically since callers may want to build the Func
/// before the Signature is wired up (matching Go's `NewFunc(.., nil)` escape
/// hatch).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Func {
    name: String,
    typ: Option<TypeId>,
    pub(crate) meta: ObjectMeta,
}

impl Func {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.typ = r.ty_opt(self.typ);
        self.meta.remap_ids(r);
    }
}

impl Func {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The function's type, expected to be a `Signature` once populated.
    /// `None` matches Go's `NewFunc(.., nil)` two-phase construction.
    pub fn typ(&self) -> Option<TypeId> {
        self.typ
    }

    pub fn set_typ(&mut self, typ: TypeId) {
        self.typ = Some(typ);
    }
}

impl HasMeta for Func {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// Reports whether the receiver of method `f` is of the form `*T`.
///
/// Equivalent to `types2.Func.hasPtrRecv()` — minus the `hasPtrRecv_`
/// early-stage flag, which only matters during type-checking before the
/// signature is wired. Returns `false` if the Func has no Signature, or the
/// Signature has no receiver.
pub fn func_has_ptr_recv(type_arena: &TypeArena, object_arena: &ObjectArena, f: ObjectId) -> bool {
    let sig_id = match object_arena.get(f) {
        ObjectData::Func(func) => match func.typ() {
            Some(t) => t,
            None => return false,
        },
        _ => return false,
    };
    let recv = match type_arena.get(sig_id) {
        TypeData::Signature(s) => s.recv(),
        _ => return false,
    };
    let recv = match recv {
        Some(r) => r,
        None => return false,
    };
    let recv_typ = match recv.typ(object_arena) {
        Some(t) => t,
        None => return false,
    };
    let resolved = crate::alias::unalias_readonly(type_arena, recv_typ);
    matches!(type_arena.get(resolved), TypeData::Pointer(_))
}

/// Construct a new [`Func`] object.
///
/// Equivalent to `types2.NewFunc`. Pass `None` for `sig` to build the Func
/// before its Signature exists (Go's "two-phase construction" pattern).
pub fn new_func(arena: &mut ObjectArena, name: impl Into<String>, sig: Option<TypeId>) -> ObjectId {
    arena.alloc(ObjectData::Func(Func {
        name: name.into(),
        typ: sig,
        meta: ObjectMeta::default(),
    }))
}
