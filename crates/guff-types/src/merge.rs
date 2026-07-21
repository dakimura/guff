//! Relocation of a worker's private overlay into a shared seed base (R25).
//!
//! Parallel dependency type-checking builds each package in its own
//! [`Checker`](crate::Checker) cloned from a shared frozen seed via
//! [`Checker::from_seed`], so every worker allocates ids starting just past the
//! same `base_len`. To fold N such overlays into one arena we concatenate them
//! and shift each worker's own ids by the total length of the overlays merged
//! before it. Ids that point into the shared base (`<= base_len`) are untouched;
//! ids into a worker's own overlay shift by that worker's `delta`.
//!
//! The invariant that makes a single additive shift correct: workers merged in
//! one wave are mutually independent (same topological level ⇒ neither imports
//! the other), so a worker's overlay references only the shared base and its own
//! overlay — never a sibling's. See `crates/guff-packages/src/typecheck.rs`.

use crate::arena::{ObjectData, ObjectId, PackageId, ScopeId, TypeData, TypeId};
use crate::package::Package;
use crate::scope::Scope;

/// Per-arena relocation parameters for one worker's overlay.
///
/// `*_base` is the shared seed's element count (ids `<= base` stay put); `*_delta`
/// is how far this worker's overlay ids move when appended after the overlays
/// that were merged before it.
pub(crate) struct Remapper {
    pub(crate) ty_base: u32,
    pub(crate) ty_delta: u32,
    pub(crate) ob_base: u32,
    pub(crate) ob_delta: u32,
    pub(crate) sc_base: u32,
    pub(crate) sc_delta: u32,
    pub(crate) pk_base: u32,
    pub(crate) pk_delta: u32,
}

impl Remapper {
    #[inline]
    pub(crate) fn ty(&self, id: TypeId) -> TypeId {
        id.remapped(self.ty_base, self.ty_delta)
    }
    #[inline]
    pub(crate) fn ty_opt(&self, id: Option<TypeId>) -> Option<TypeId> {
        id.map(|i| self.ty(i))
    }
    #[inline]
    pub(crate) fn obj(&self, id: ObjectId) -> ObjectId {
        id.remapped(self.ob_base, self.ob_delta)
    }
    #[inline]
    pub(crate) fn obj_opt(&self, id: Option<ObjectId>) -> Option<ObjectId> {
        id.map(|i| self.obj(i))
    }
    #[inline]
    pub(crate) fn scope(&self, id: ScopeId) -> ScopeId {
        id.remapped(self.sc_base, self.sc_delta)
    }
    #[inline]
    pub(crate) fn scope_opt(&self, id: Option<ScopeId>) -> Option<ScopeId> {
        id.map(|i| self.scope(i))
    }
    #[inline]
    pub(crate) fn pkg(&self, id: PackageId) -> PackageId {
        id.remapped(self.pk_base, self.pk_delta)
    }
    #[inline]
    pub(crate) fn pkg_opt(&self, id: Option<PackageId>) -> Option<PackageId> {
        id.map(|i| self.pkg(i))
    }
}

/// Relocate every id inside one type. Dispatch to the per-variant methods that
/// live in each type's own module (so private fields stay encapsulated).
pub(crate) fn remap_type(t: &mut TypeData, r: &Remapper) {
    match t {
        TypeData::Basic(_) => {}
        TypeData::Array(a) => a.remap_ids(r),
        TypeData::Slice(s) => s.remap_ids(r),
        TypeData::Pointer(p) => p.remap_ids(r),
        TypeData::Map(m) => m.remap_ids(r),
        TypeData::Chan(c) => c.remap_ids(r),
        TypeData::Tuple(t) => t.remap_ids(r),
        TypeData::Struct(s) => s.remap_ids(r),
        TypeData::Signature(s) => s.remap_ids(r),
        TypeData::Interface(i) => i.remap_ids(r),
        TypeData::Union(u) => u.remap_ids(r),
        TypeData::Named(n) => n.remap_ids(r),
        TypeData::Alias(a) => a.remap_ids(r),
        TypeData::TypeParam(tp) => tp.remap_ids(r),
    }
}

/// Relocate every id inside one object.
pub(crate) fn remap_object(o: &mut ObjectData, r: &Remapper) {
    match o {
        ObjectData::Var(v) => v.remap_ids(r),
        ObjectData::Func(f) => f.remap_ids(r),
        ObjectData::TypeName(t) => t.remap_ids(r),
        ObjectData::Const(c) => c.remap_ids(r),
        ObjectData::Nil(n) => n.remap_ids(r),
        ObjectData::Builtin(_) => {}
        ObjectData::PkgName(p) => p.remap_ids(r),
    }
}

/// Relocate every id inside one scope.
pub(crate) fn remap_scope(s: &mut Scope, r: &Remapper) {
    s.remap_ids(r);
}

/// Relocate every id inside one package.
pub(crate) fn remap_package(p: &mut Package, r: &Remapper) {
    p.remap_ids(r);
}
