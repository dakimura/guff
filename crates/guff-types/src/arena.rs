//! Arena-based storage for Go types and objects.
//!
//! Types and objects form a cyclic, mutable graph in `go/types` — Named's
//! underlying may be a Struct whose fields are Vars whose `typ` may point back
//! to that Named. We model this with two arenas (`TypeArena`, `ObjectArena`)
//! and `TypeId` / `ObjectId` indices in place of Go's `*T` pointers.
//!
//! - `TypeId` and `ObjectId` use `NonZeroU32` so `Option<TypeId>` stays 4
//!   bytes (matching the size of a bare ID and avoiding tag overhead).
//! - IDs are 1-indexed internally; index 0 is reserved as the niche.

use std::num::NonZeroU32;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::alias::Alias;
use crate::array::Array;
use crate::basic::Basic;
use crate::chan::Chan;
use crate::interface::Interface;
use crate::map::Map;
use crate::named::Named;
use crate::object::builtin::Builtin;
use crate::object::const_::Const;
use crate::object::func::Func;
use crate::object::nil_::Nil;
use crate::object::pkgname::PkgName;
use crate::object::type_name::TypeName;
use crate::object::var::Var;
use crate::package::Package;
use crate::pointer::Pointer;
use crate::r#struct::Struct;
use crate::scope::Scope;
use crate::signature::Signature;
use crate::slice::Slice;
use crate::tuple::Tuple;
use crate::typeparam::TypeParam;
use crate::union::Union;

/// Handle to a type stored in a [`TypeArena`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TypeId(NonZeroU32);

impl TypeId {
    /// Construct a `TypeId` from a 1-based arena index. Crate-internal helper
    /// for the few places that iterate an arena by position (e.g.
    /// [`crate::basic::lookup_basic`]).
    ///
    /// # Panics
    /// Panics if `index` is 0.
    pub(crate) fn from_index(index: usize) -> Self {
        TypeId(NonZeroU32::new(index as u32).expect("arena index never 0"))
    }

    /// Relocate an id when merging a worker overlay into a shared base (R25).
    /// Ids `<= base_len` point into the shared frozen base and are unchanged;
    /// worker-local ids (into the overlay) shift by `delta` elements.
    #[inline]
    pub(crate) fn remapped(self, base_len: u32, delta: u32) -> Self {
        if self.0.get() <= base_len {
            self
        } else {
            TypeId(NonZeroU32::new(self.0.get() + delta).expect("remap never 0"))
        }
    }
}

/// Handle to an object stored in an [`ObjectArena`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(NonZeroU32);

impl ObjectId {
    /// See [`TypeId::remapped`].
    #[inline]
    pub(crate) fn remapped(self, base_len: u32, delta: u32) -> Self {
        if self.0.get() <= base_len {
            self
        } else {
            ObjectId(NonZeroU32::new(self.0.get() + delta).expect("remap never 0"))
        }
    }
}

/// Handle to a [`Scope`] stored in a [`ScopeArena`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScopeId(NonZeroU32);

impl ScopeId {
    /// See [`TypeId::remapped`].
    #[inline]
    pub(crate) fn remapped(self, base_len: u32, delta: u32) -> Self {
        if self.0.get() <= base_len {
            self
        } else {
            ScopeId(NonZeroU32::new(self.0.get() + delta).expect("remap never 0"))
        }
    }
}

/// Handle to a [`Package`] stored in a [`PackageArena`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PackageId(NonZeroU32);

impl PackageId {
    /// See [`TypeId::remapped`].
    #[inline]
    pub(crate) fn remapped(self, base_len: u32, delta: u32) -> Self {
        if self.0.get() <= base_len {
            self
        } else {
            PackageId(NonZeroU32::new(self.0.get() + delta).expect("remap never 0"))
        }
    }
}

/// Storage for all Go types in a type-checking session. Types reference each
/// other by [`TypeId`]; the arena owns the underlying data.
///
/// Mutation is performed via [`TypeArena::get_mut`]. Concurrent access is not
/// supported (matching `go/types`' single-threaded `Checker`).
#[derive(Debug, Default, Clone)]
pub struct TypeArena {
    types: Layered<TypeData>,
}

/// Storage for all Go objects (variables, functions, type names, etc.).
#[derive(Debug, Default, Clone)]
pub struct ObjectArena {
    objects: Layered<ObjectData>,
}

/// Storage for all [`Scope`]s in a type-checking session.
#[derive(Debug, Default, Clone)]
pub struct ScopeArena {
    scopes: Layered<Scope>,
}

/// Storage for all [`Package`]s in a type-checking session.
#[derive(Debug, Default, Clone)]
pub struct PackageArena {
    packages: Layered<Package>,
}

/// A copy-on-write, append-friendly backing store shared across arena clones.
///
/// The `base` prefix is an `Arc`-shared, effectively read-only run of elements;
/// `overlay` holds elements appended after the base was frozen. Cloning shares
/// the base (an `Arc` refcount bump) and deep-copies only the usually-small
/// overlay, so the large decoded-dependency prefix (the R24.3 export seed) is
/// shared across all packages instead of duplicated per package. Element ids are
/// stable positions into `base` then `overlay`, so existing ids keep working as
/// the overlay grows.
///
/// Mutating a base element first promotes the base to a private copy
/// (`Arc::make_mut`); this is rare in practice — measured on Prometheus, only a
/// handful of packages mutate a base type during type-checking and none during
/// SSA construction — so the shared prefix survives for almost every clone.
#[derive(Debug, Clone)]
struct Layered<T> {
    base: Arc<Vec<T>>,
    overlay: Vec<T>,
}

impl<T> Default for Layered<T> {
    fn default() -> Self {
        Self {
            base: Arc::new(Vec::new()),
            overlay: Vec::new(),
        }
    }
}

impl<T: Clone> Layered<T> {
    #[inline]
    fn len(&self) -> usize {
        self.base.len() + self.overlay.len()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.base.is_empty() && self.overlay.is_empty()
    }

    #[inline]
    fn get(&self, idx: usize) -> &T {
        let b = self.base.len();
        if idx < b {
            &self.base[idx]
        } else {
            &self.overlay[idx - b]
        }
    }

    #[inline]
    fn get_mut(&mut self, idx: usize) -> &mut T {
        let b = self.base.len();
        if idx < b {
            // Copy-on-write: promote the shared base to a private copy so we can
            // mutate element `idx`. Cheap and one-shot per arena (subsequent base
            // mutations reuse the now-owned copy).
            &mut Arc::make_mut(&mut self.base)[idx]
        } else {
            &mut self.overlay[idx - b]
        }
    }

    /// Append `data`, returning its 0-based index. Appends always land in the
    /// owned overlay, so they never disturb the shared base.
    #[inline]
    fn push(&mut self, data: T) -> usize {
        let idx = self.len();
        self.overlay.push(data);
        idx
    }

    /// Fold the overlay into the base so the whole store can subsequently be
    /// shared read-only via [`Layered::shared_clone`].
    fn freeze(&mut self) {
        if !self.overlay.is_empty() {
            let base = Arc::make_mut(&mut self.base);
            base.append(&mut self.overlay);
        }
    }

    /// Number of elements appended after the base was shared (the worker's own
    /// contribution). Used to size the per-worker relocation delta (R25).
    #[inline]
    fn overlay_len(&self) -> usize {
        self.overlay.len()
    }

    /// Consume the store and return only the overlay, dropping the (possibly
    /// copy-on-write-diverged) base. Used to extract a finished worker's own
    /// allocations for merging into a shared seed (R25); the base is the shared
    /// frozen seed the worker was cloned from and is discarded.
    fn into_overlay(self) -> Vec<T> {
        self.overlay
    }

    /// Append already-relocated elements directly into the base. The caller must
    /// hold the only reference to the base (all worker clones dropped) so
    /// `Arc::make_mut` mutates in place. Keeps the overlay empty so the store
    /// stays shareable via [`Layered::shared_clone`] (R25).
    fn extend_base(&mut self, items: Vec<T>) {
        debug_assert!(
            self.overlay.is_empty(),
            "extend_base requires a frozen store (empty overlay)"
        );
        let base = Arc::make_mut(&mut self.base);
        base.extend(items);
    }

    /// Share the (frozen) base with a fresh empty overlay — an `Arc` refcount
    /// bump, no element copies. Requires [`Layered::freeze`] to have run.
    fn shared_clone(&self) -> Self {
        debug_assert!(
            self.overlay.is_empty(),
            "shared_clone requires a frozen arena (empty overlay)"
        );
        Self {
            base: Arc::clone(&self.base),
            overlay: Vec::new(),
        }
    }
}

/// Backing data for each [`TypeId`]. One variant per Go type kind.
///
/// Chunks 1–3 cover every type kind. The Checker proper (which animates them)
/// is still to come.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeData {
    Basic(Basic),
    Array(Array),
    Slice(Slice),
    Pointer(Pointer),
    Map(Map),
    Chan(Chan),
    Tuple(Tuple),
    Struct(Struct),
    Signature(Signature),
    Interface(Interface),
    Union(Union),
    Named(Named),
    Alias(Alias),
    TypeParam(TypeParam),
}

/// Backing data for each [`ObjectId`].
///
/// Chunks 1–6 cover `Var`, `Func`, `TypeName`, `Const`, `Nil`, `Builtin`.
/// `PkgName` arrives with imports (D16). `Label` is still deferred.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObjectData {
    Var(Var),
    Func(Func),
    TypeName(TypeName),
    Const(Const),
    Nil(Nil),
    Builtin(Builtin),
    PkgName(PkgName),
}

impl TypeArena {
    /// Create an empty arena. To get the predeclared basic types as well, use
    /// [`crate::basic::init_universe`] which returns a populated arena plus
    /// the lookup table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert `data` and return a stable [`TypeId`] pointing to it.
    pub fn alloc(&mut self, data: TypeData) -> TypeId {
        // Index is 1-based so Option<TypeId> can use 0 as the niche.
        let raw = (self.types.push(data) + 1) as u32;
        TypeId(NonZeroU32::new(raw).expect("arena index never 0"))
    }

    pub fn get(&self, id: TypeId) -> &TypeData {
        self.types.get((id.0.get() - 1) as usize)
    }

    pub fn get_mut(&mut self, id: TypeId) -> &mut TypeData {
        self.types.get_mut((id.0.get() - 1) as usize)
    }

    pub fn len(&self) -> usize {
        self.types.len()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// Fold appended types into the shared base so this arena can be shared
    /// read-only across packages (see [`TypeArena::shared_clone`]).
    pub fn freeze(&mut self) {
        self.types.freeze();
    }

    /// Clone sharing the frozen base (an `Arc` bump, no element copies).
    pub fn shared_clone(&self) -> Self {
        Self {
            types: self.types.shared_clone(),
        }
    }

    /// Overlay length — this worker's own allocations (R25).
    pub(crate) fn overlay_len(&self) -> usize {
        self.types.overlay_len()
    }

    /// Consume and return the overlay, discarding the shared base (R25).
    pub(crate) fn into_overlay(self) -> Vec<TypeData> {
        self.types.into_overlay()
    }

    /// Append relocated elements into the base (R25).
    pub(crate) fn extend_base(&mut self, items: Vec<TypeData>) {
        self.types.extend_base(items);
    }
}

impl ObjectArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn alloc(&mut self, data: ObjectData) -> ObjectId {
        let raw = (self.objects.push(data) + 1) as u32;
        ObjectId(NonZeroU32::new(raw).expect("arena index never 0"))
    }

    pub fn get(&self, id: ObjectId) -> &ObjectData {
        self.objects.get((id.0.get() - 1) as usize)
    }

    pub fn get_mut(&mut self, id: ObjectId) -> &mut ObjectData {
        self.objects.get_mut((id.0.get() - 1) as usize)
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Iterates all allocated object ids in creation order.
    pub fn ids(&self) -> impl Iterator<Item = ObjectId> + '_ {
        (0..self.len()).map(|i| ObjectId(NonZeroU32::new((i + 1) as u32).expect("object id")))
    }

    /// Fold appended objects into the shared base (see [`ObjectArena::shared_clone`]).
    pub fn freeze(&mut self) {
        self.objects.freeze();
    }

    /// Clone sharing the frozen base (an `Arc` bump, no element copies).
    pub fn shared_clone(&self) -> Self {
        Self {
            objects: self.objects.shared_clone(),
        }
    }

    /// Overlay length — this worker's own allocations (R25).
    pub(crate) fn overlay_len(&self) -> usize {
        self.objects.overlay_len()
    }

    /// Consume and return the overlay, discarding the shared base (R25).
    pub(crate) fn into_overlay(self) -> Vec<ObjectData> {
        self.objects.into_overlay()
    }

    /// Append relocated elements into the base (R25).
    pub(crate) fn extend_base(&mut self, items: Vec<ObjectData>) {
        self.objects.extend_base(items);
    }
}

impl ScopeArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn alloc(&mut self, data: Scope) -> ScopeId {
        let raw = (self.scopes.push(data) + 1) as u32;
        ScopeId(NonZeroU32::new(raw).expect("arena index never 0"))
    }

    pub fn get(&self, id: ScopeId) -> &Scope {
        self.scopes.get((id.0.get() - 1) as usize)
    }

    pub fn get_mut(&mut self, id: ScopeId) -> &mut Scope {
        self.scopes.get_mut((id.0.get() - 1) as usize)
    }

    pub fn len(&self) -> usize {
        self.scopes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }

    /// Fold appended scopes into the shared base (see [`ScopeArena::shared_clone`]).
    pub fn freeze(&mut self) {
        self.scopes.freeze();
    }

    /// Clone sharing the frozen base (an `Arc` bump, no element copies).
    pub fn shared_clone(&self) -> Self {
        Self {
            scopes: self.scopes.shared_clone(),
        }
    }

    /// Overlay length — this worker's own allocations (R25).
    pub(crate) fn overlay_len(&self) -> usize {
        self.scopes.overlay_len()
    }

    /// Consume and return the overlay, discarding the shared base (R25).
    pub(crate) fn into_overlay(self) -> Vec<Scope> {
        self.scopes.into_overlay()
    }

    /// Append relocated elements into the base (R25).
    pub(crate) fn extend_base(&mut self, items: Vec<Scope>) {
        self.scopes.extend_base(items);
    }
}

impl PackageArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn alloc(&mut self, data: Package) -> PackageId {
        let raw = (self.packages.push(data) + 1) as u32;
        PackageId(NonZeroU32::new(raw).expect("arena index never 0"))
    }

    pub fn get(&self, id: PackageId) -> &Package {
        self.packages.get((id.0.get() - 1) as usize)
    }

    pub fn get_mut(&mut self, id: PackageId) -> &mut Package {
        self.packages.get_mut((id.0.get() - 1) as usize)
    }

    pub fn len(&self) -> usize {
        self.packages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }

    /// Fold appended packages into the shared base (see [`PackageArena::shared_clone`]).
    pub fn freeze(&mut self) {
        self.packages.freeze();
    }

    /// Clone sharing the frozen base (an `Arc` bump, no element copies).
    pub fn shared_clone(&self) -> Self {
        Self {
            packages: self.packages.shared_clone(),
        }
    }

    /// Overlay length — this worker's own allocations (R25).
    pub(crate) fn overlay_len(&self) -> usize {
        self.packages.overlay_len()
    }

    /// Consume and return the overlay, discarding the shared base (R25).
    pub(crate) fn into_overlay(self) -> Vec<Package> {
        self.packages.into_overlay()
    }

    /// Append relocated elements into the base (R25).
    pub(crate) fn extend_base(&mut self, items: Vec<Package>) {
        self.packages.extend_base(items);
    }

    /// Return the package id at `index` (0-based arena position).
    pub fn id_at(&self, index: usize) -> PackageId {
        PackageId(NonZeroU32::new((index + 1) as u32).expect("arena index never 0"))
    }

    /// Look up a package by its import path.
    pub fn find_by_path(&self, path: &str) -> Option<PackageId> {
        (0..self.len()).find_map(|i| {
            let id = self.id_at(i);
            (self.get(id).path() == path).then_some(id)
        })
    }
}
