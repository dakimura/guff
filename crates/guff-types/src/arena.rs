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
use crate::chan::{Chan, ChanDir};
use crate::hash::HashMap;
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
///
/// Structural types (Pointer / Slice / Array / Map / Chan / Signature) are
/// hash-consed via [`InternKey`] so identical shapes reuse one [`TypeId`]
/// (B-5). Named / Interface / etc. are never interned.
#[derive(Debug, Default, Clone)]
pub struct TypeArena {
    types: Layered<TypeData>,
    /// Intern table for the frozen base (shared across [`TypeArena::shared_clone`]).
    intern_base: Arc<HashMap<InternKey, TypeId>>,
    /// Intern table for types appended after the last freeze / shared_clone.
    ///
    /// `Arc` for the same reason as [`Layered::overlay`] — see V1-1 there. A
    /// scratch clone that asks a question without interning never touches it.
    intern_overlay: Arc<HashMap<InternKey, TypeId>>,
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
/// `overlay` holds elements appended after the base was frozen. Element ids are
/// stable positions into `base` then `overlay`, so existing ids keep working as
/// the overlay grows.
///
/// **Both layers are `Arc`, so cloning is two refcount bumps regardless of size**
/// (PERF_TASKS_V3 V1-1). The overlay used to be an owned `Vec`, which made
/// `TypeArena::clone` deep-copy every type the package had allocated. That
/// mattered because `identical` / `implements` / `lookup_field_or_method` take
/// `&mut TypeArena` (interning can append), so ~30 check sites clone the arena
/// *per call* just to ask a question:
///
/// ```ignore
/// let mut types = artifacts.types.clone();   // was: whole overlay, per call
/// identical(&mut types, &artifacts.objects, &artifacts.packages, a, b)
/// ```
///
/// On prometheus `./...` that put `TypeArena::clone` at 1.36s of self CPU (6.8%
/// of the run), before counting the `memmove`, the allocator traffic and the
/// `Vec<TypeData>` drop it dragged behind it. With a shared overlay the clone is
/// free and the copy happens only if the callee actually appends — which is the
/// rare case, and costs exactly what the unconditional clone used to.
///
/// Mutating a base element first promotes the base to a private copy
/// (`Arc::make_mut`); this is rare in practice — measured on Prometheus, only a
/// handful of packages mutate a base type during type-checking and none during
/// SSA construction — so the shared prefix survives for almost every clone.
#[derive(Debug, Clone)]
struct Layered<T> {
    base: Arc<Vec<T>>,
    overlay: Arc<Vec<T>>,
}

impl<T> Default for Layered<T> {
    fn default() -> Self {
        Self {
            base: Arc::new(Vec::new()),
            overlay: Arc::new(Vec::new()),
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
            &mut Arc::make_mut(&mut self.overlay)[idx - b]
        }
    }

    /// Append `data`, returning its 0-based index. Appends always land in the
    /// overlay, so they never disturb the shared base.
    ///
    /// `Arc::make_mut` is a uniqueness check, not a copy, on the type-checking
    /// path: the checker owns its overlay alone while it builds a package. It
    /// only copies when a *scratch* clone appends — exactly the case the old
    /// unconditional deep clone paid for every time.
    #[inline]
    fn push(&mut self, data: T) -> usize {
        let idx = self.len();
        Arc::make_mut(&mut self.overlay).push(data);
        idx
    }

    /// Fold the overlay into the base so the whole store can subsequently be
    /// shared read-only via [`Layered::shared_clone`].
    fn freeze(&mut self) {
        if !self.overlay.is_empty() {
            let base = Arc::make_mut(&mut self.base);
            base.append(Arc::make_mut(&mut self.overlay));
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
        // Uniquely owned on this path (the worker is finished and its scratch
        // clones are gone), so this unwraps without copying.
        Arc::try_unwrap(self.overlay).unwrap_or_else(|shared| (*shared).clone())
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
            overlay: Arc::new(Vec::new()),
        }
    }

    /// Borrow the shared base and the overlay (RSS attribution, C-8).
    ///
    /// The overlay is `Arc`-shared since V1-1, so a scratch clone that never
    /// appended charges its (shared) overlay again here. That over-counts only
    /// under `GUFF_DEBUG_RSS`, and only while a scratch clone is alive.
    pub(crate) fn parts(&self) -> (&Arc<Vec<T>>, &Vec<T>) {
        (&self.base, &self.overlay)
    }
}

/// Backing data for each [`TypeId`]. One variant per Go type kind.
///
/// Chunks 1–3 cover every type kind. The Checker proper (which animates them)
/// is still to come.
///
/// **This enum's size is multiplied by about 5.1M** on prometheus `./...` —
/// it is the single largest thing the process retains (PERF_TASKS_V9 §V9-3).
/// The arena is dominated by the *small* kinds: a `Slice` is 4 bytes and a
/// `Pointer` is 4, and they were each occupying 112 because `Interface` (112)
/// and `Named` (96) are inline. Boxing those two takes the slot to 72 and peak
/// RSS down 150 MiB, with wall and CPU unchanged across two interleaved A/B
/// runs — the indirection is only paid when an interface or named type is read,
/// and the dense kinds got 36% more of them per cache line.
///
/// `Box` is transparent to serde, so seed overlays on disk are unaffected.
///
/// So: **check `size_of::<TypeData>()` before adding or widening a variant.**
/// [`TYPE_DATA_STAYS_SMALL`] fails the build if it grows.
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
    /// Boxed to keep [`TypeData`] small — see the type's docs before unboxing.
    Interface(Box<Interface>),
    Union(Union),
    /// Boxed to keep [`TypeData`] small — see the type's docs before unboxing.
    Named(Box<Named>),
    Alias(Alias),
    TypeParam(TypeParam),
}

/// Guards the slot size the type arena was tuned to (see [`TypeData`]).
///
/// A ceiling, not an equality: a new small variant stays a non-event. If this
/// fires, `Box` the variant that grew rather than raising the number — every
/// one of ~5.1M slots pays the difference, whether or not it is that kind.
const TYPE_DATA_STAYS_SMALL: () = assert!(
    std::mem::size_of::<TypeData>() <= 80,
    "TypeData grew past 80 bytes; ~5.1M arena slots pay for it (docs/PERF_TASKS_V9.md §V9-3)",
);
const _: () = TYPE_DATA_STAYS_SMALL;

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

/// Shallow structural key for B-5 hash-consing. Pointer / Array / Map / Chan /
/// Signature only — **not Slice**.
///
/// Slice is excluded because some analyzers (revive `var-declaration`) compare
/// `TypeId`s with `==` instead of structural [`identical`](crate::predicates::identical).
/// Interning `[]T` makes the LHS type-expr and RHS composite-lit share an id and
/// flips findings vs golangci on prometheus. Pointer/Array/Map/Chan do not have
/// that regression in the current suite.
///
/// Signature includes `recv` so two methods with the same params/results but
/// different receivers do not incorrectly share a `TypeId` (the `recv` field
/// would otherwise be lost on reuse).
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
enum InternKey {
    Pointer(TypeId),
    Array { len: i64, elem: TypeId },
    Map { key: TypeId, elem: TypeId },
    Chan { dir: ChanDir, elem: TypeId },
    Signature {
        recv: Option<ObjectId>,
        params: Option<TypeId>,
        results: Option<TypeId>,
        variadic: bool,
        rparams: Vec<TypeId>,
        tparams: Vec<TypeId>,
    },
}

impl InternKey {
    fn from_data(data: &TypeData) -> Option<Self> {
        Some(match data {
            TypeData::Pointer(p) => Self::Pointer(p.elem()),
            TypeData::Array(a) => Self::Array {
                len: a.len(),
                elem: a.elem(),
            },
            TypeData::Map(m) => Self::Map {
                key: m.key(),
                elem: m.elem(),
            },
            TypeData::Chan(c) => Self::Chan {
                dir: c.dir(),
                elem: c.elem(),
            },
            TypeData::Signature(s) => Self::Signature {
                recv: s.recv(),
                params: s.params(),
                results: s.results(),
                variadic: s.variadic(),
                rparams: s
                    .recv_type_params()
                    .map(|l| l.list().to_vec())
                    .unwrap_or_default(),
                tparams: s
                    .type_params()
                    .map(|l| l.list().to_vec())
                    .unwrap_or_default(),
            },
            // Slice intentionally not interned — see InternKey docs.
            TypeData::Slice(_) => return None,
            _ => return None,
        })
    }
}

/// B-5 GO/NO-GO: counts of structural types and how many unique shallow keys
/// they would collapse to under hash-consing. Each pair is `(count, unique)`.
#[derive(Debug, Clone, Copy)]
pub struct StructuralDupStats {
    pub total_types: usize,
    pub structural: usize,
    pub unique_structural: usize,
    pub pointer: (usize, usize),
    pub slice: (usize, usize),
    pub array: (usize, usize),
    pub map: (usize, usize),
    pub chan: (usize, usize),
    pub signature: (usize, usize),
}

impl StructuralDupStats {
    /// `1 - unique/structural`. Zero when there are no structural types.
    pub fn dup_rate(&self) -> f64 {
        if self.structural == 0 {
            0.0
        } else {
            1.0 - (self.unique_structural as f64 / self.structural as f64)
        }
    }
}

impl TypeArena {
    /// Create an empty arena. To get the predeclared basic types as well, use
    /// [`crate::basic::init_universe`] which returns a populated arena plus
    /// the lookup table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert `data` and return a stable [`TypeId`] pointing to it.
    ///
    /// Structural shapes (Pointer / Array / Map / Chan / Signature — not Slice)
    /// are hash-consed. Lookup checks overlay then frozen base.
    pub fn alloc(&mut self, data: TypeData) -> TypeId {
        if let Some(key) = InternKey::from_data(&data) {
            if let Some(&id) = self.intern_overlay.get(&key) {
                return id;
            }
            if let Some(&id) = self.intern_base.get(&key) {
                return id;
            }
            let id = self.alloc_fresh(data);
            Arc::make_mut(&mut self.intern_overlay).insert(key, id);
            return id;
        }
        self.alloc_fresh(data)
    }

    /// Re-key the hash-cons entry for `id` around an in-place mutation.
    ///
    /// The intern table is keyed by structure, so mutating an interned type
    /// leaves the old key pointing at a type that no longer has that shape —
    /// and the next `alloc` of something that *does* have that shape gets the
    /// mutated type back. A signature is built before its type parameters are
    /// known ([`crate::signature::signature_set_type_params`]), which is
    /// exactly this: `func[T any]() error` is interned as `func() error`, and
    /// instantiating it then hands back the generic original.
    ///
    /// Only the overlay is repaired; the frozen base holds types from packages
    /// that finished checking, which are never mutated again.
    pub fn remutate<R>(&mut self, id: TypeId, f: impl FnOnce(&mut TypeData) -> R) -> R {
        let old_key = InternKey::from_data(self.get(id));
        if let Some(key) = old_key {
            if self.intern_overlay.get(&key) == Some(&id) {
                Arc::make_mut(&mut self.intern_overlay).remove(&key);
            }
        }
        // The new shape is deliberately *not* re-interned. Hash-consing is an
        // optimization and dropping an entry only costs sharing, while this
        // path runs for every generic signature in the program (including every
        // one read back from export data) — so the cheaper half is the one that
        // keeps the table correct.
        f(self.get_mut(id))
    }

    fn alloc_fresh(&mut self, data: TypeData) -> TypeId {
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

    /// Shallow structural-duplicate stats for B-5 (Pointer / Slice / Array /
    /// Map / Chan / Signature). Slice is counted even though it is not interned.
    pub fn structural_dup_stats(&self) -> StructuralDupStats {
        #[derive(Hash, Eq, PartialEq)]
        enum StatKey {
            Intern(InternKey),
            Slice(TypeId),
        }

        let mut seen: HashMap<StatKey, u32> = HashMap::default();
        let mut kind_n = [0usize; 6];
        let mut kind_unique = [0usize; 6];

        for i in 1..=self.len() {
            let id = TypeId::from_index(i);
            let (kind, key) = match self.get(id) {
                TypeData::Pointer(p) => (0, StatKey::Intern(InternKey::Pointer(p.elem()))),
                TypeData::Slice(s) => (1, StatKey::Slice(s.elem())),
                TypeData::Array(a) => (
                    2,
                    StatKey::Intern(InternKey::Array {
                        len: a.len(),
                        elem: a.elem(),
                    }),
                ),
                TypeData::Map(m) => (
                    3,
                    StatKey::Intern(InternKey::Map {
                        key: m.key(),
                        elem: m.elem(),
                    }),
                ),
                TypeData::Chan(c) => (
                    4,
                    StatKey::Intern(InternKey::Chan {
                        dir: c.dir(),
                        elem: c.elem(),
                    }),
                ),
                TypeData::Signature(s) => (
                    5,
                    StatKey::Intern(InternKey::Signature {
                        recv: s.recv(),
                        params: s.params(),
                        results: s.results(),
                        variadic: s.variadic(),
                        rparams: s
                            .recv_type_params()
                            .map(|l| l.list().to_vec())
                            .unwrap_or_default(),
                        tparams: s
                            .type_params()
                            .map(|l| l.list().to_vec())
                            .unwrap_or_default(),
                    }),
                ),
                _ => continue,
            };
            kind_n[kind] += 1;
            let e = seen.entry(key).or_insert(0);
            if *e == 0 {
                kind_unique[kind] += 1;
            }
            *e += 1;
        }

        let structural: usize = kind_n.iter().sum();
        let unique_structural: usize = kind_unique.iter().sum();
        StructuralDupStats {
            total_types: self.len(),
            structural,
            unique_structural,
            pointer: (kind_n[0], kind_unique[0]),
            slice: (kind_n[1], kind_unique[1]),
            array: (kind_n[2], kind_unique[2]),
            map: (kind_n[3], kind_unique[3]),
            chan: (kind_n[4], kind_unique[4]),
            signature: (kind_n[5], kind_unique[5]),
        }
    }

    /// Fold appended types into the shared base so this arena can be shared
    /// read-only across packages (see [`TypeArena::shared_clone`]).
    pub fn freeze(&mut self) {
        self.types.freeze();
        if !self.intern_overlay.is_empty() {
            let base = Arc::make_mut(&mut self.intern_base);
            for (k, v) in Arc::make_mut(&mut self.intern_overlay).drain() {
                base.entry(k).or_insert(v);
            }
        }
    }

    /// Clone sharing the frozen base (an `Arc` bump, no element copies).
    pub fn shared_clone(&self) -> Self {
        Self {
            types: self.types.shared_clone(),
            intern_base: Arc::clone(&self.intern_base),
            intern_overlay: Arc::new(HashMap::default()),
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
    ///
    /// Structural keys that are already interned stay pointing at the first id;
    /// newly seen keys are registered. Parallel-wave duplicates therefore remain
    /// in the arena but do not poison future lookups.
    pub(crate) fn extend_base(&mut self, items: Vec<TypeData>) {
        let start = self.types.len();
        self.types.extend_base(items);
        let intern = Arc::make_mut(&mut self.intern_base);
        for i in start..self.types.len() {
            let id = TypeId::from_index(i + 1);
            if let Some(key) = InternKey::from_data(self.types.get(i)) {
                intern.entry(key).or_insert(id);
            }
        }
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

// ---- RSS attribution (PERF_TASKS_V2 C-8) ------------------------------------

impl TypeArena {
    /// Charge this arena's slot storage into `acct` (Arc base deduped).
    pub fn account_retained(&self, acct: &mut crate::retained::RetainedBytes) {
        self.account_retained_inner(acct, false);
    }

    /// Owned overlay only (SSA incremental cost on top of a shared base).
    pub fn account_overlay_only(&self, acct: &mut crate::retained::RetainedBytes) {
        self.account_retained_inner(acct, true);
    }

    fn account_retained_inner(&self, acct: &mut crate::retained::RetainedBytes, overlay_only: bool) {
        use crate::retained::{account_arc_map_approx, account_arc_vec_slots, account_owned_map_approx};
        use std::mem::size_of;

        let (base, overlay) = self.types.parts();
        if overlay_only {
            acct.type_slots = acct
                .type_slots
                .saturating_add(overlay.capacity().saturating_mul(size_of::<TypeData>()));
        } else {
            account_arc_vec_slots(
                &mut acct.seen_ptrs,
                base,
                overlay.capacity(),
                &mut acct.type_slots,
            );
            account_arc_map_approx(&mut acct.seen_ptrs, &self.intern_base, &mut acct.intern_tables);
        }
        account_owned_map_approx(&self.intern_overlay, &mut acct.intern_tables);
    }

    /// Bytes in the owned overlay only (no Arc base).
    pub fn overlay_slot_bytes(&self) -> usize {
        let (_, overlay) = self.types.parts();
        overlay.capacity().saturating_mul(std::mem::size_of::<TypeData>())
    }
}

impl ObjectArena {
    pub fn account_retained(&self, acct: &mut crate::retained::RetainedBytes) {
        self.account_retained_inner(acct, false);
    }

    pub fn account_overlay_only(&self, acct: &mut crate::retained::RetainedBytes) {
        self.account_retained_inner(acct, true);
    }

    fn account_retained_inner(&self, acct: &mut crate::retained::RetainedBytes, overlay_only: bool) {
        use crate::retained::account_arc_vec_slots;
        use std::mem::size_of;

        let (base, overlay) = self.objects.parts();
        let name_len = |obj: &ObjectData| -> usize {
            match obj {
                ObjectData::Var(v) => v.name().len(),
                ObjectData::Func(f) => f.name().len(),
                ObjectData::TypeName(t) => t.name().len(),
                ObjectData::Const(c) => c.name().len(),
                ObjectData::Nil(n) => n.name().len(),
                ObjectData::Builtin(b) => b.name().len(),
                ObjectData::PkgName(p) => p.name().len(),
            }
        };
        if overlay_only {
            acct.object_slots = acct
                .object_slots
                .saturating_add(overlay.capacity().saturating_mul(size_of::<ObjectData>()));
            for obj in overlay.iter() {
                acct.name_bytes = acct.name_bytes.saturating_add(name_len(obj));
            }
            return;
        }
        let first = account_arc_vec_slots(
            &mut acct.seen_ptrs,
            base,
            overlay.capacity(),
            &mut acct.object_slots,
        );
        if first {
            for obj in base.iter() {
                acct.name_bytes = acct.name_bytes.saturating_add(name_len(obj));
            }
        }
        for obj in overlay.iter() {
            acct.name_bytes = acct.name_bytes.saturating_add(name_len(obj));
        }
    }

    pub fn overlay_slot_bytes(&self) -> usize {
        let (_, overlay) = self.objects.parts();
        overlay.capacity().saturating_mul(std::mem::size_of::<ObjectData>())
    }
}

impl ScopeArena {
    pub fn account_retained(&self, acct: &mut crate::retained::RetainedBytes) {
        self.account_retained_inner(acct, false);
    }

    pub fn account_overlay_only(&self, acct: &mut crate::retained::RetainedBytes) {
        self.account_retained_inner(acct, true);
    }

    fn account_retained_inner(&self, acct: &mut crate::retained::RetainedBytes, overlay_only: bool) {
        use crate::retained::account_arc_vec_slots;
        use crate::scope::Scope;
        use std::mem::size_of;

        let (base, overlay) = self.scopes.parts();
        if overlay_only {
            acct.scope_slots = acct
                .scope_slots
                .saturating_add(overlay.capacity().saturating_mul(size_of::<Scope>()));
            return;
        }
        account_arc_vec_slots(
            &mut acct.seen_ptrs,
            base,
            overlay.capacity(),
            &mut acct.scope_slots,
        );
    }
}

impl PackageArena {
    pub fn account_retained(&self, acct: &mut crate::retained::RetainedBytes) {
        self.account_retained_inner(acct, false);
    }

    pub fn account_overlay_only(&self, acct: &mut crate::retained::RetainedBytes) {
        self.account_retained_inner(acct, true);
    }

    fn account_retained_inner(&self, acct: &mut crate::retained::RetainedBytes, overlay_only: bool) {
        use crate::retained::account_arc_vec_slots;
        use std::mem::size_of;

        let (base, overlay) = self.packages.parts();
        let path_bytes = |p: &Package| p.path().len().saturating_add(p.name().len());
        if overlay_only {
            acct.package_slots = acct
                .package_slots
                .saturating_add(overlay.capacity().saturating_mul(size_of::<Package>()));
            for p in overlay.iter() {
                acct.name_bytes = acct.name_bytes.saturating_add(path_bytes(p));
            }
            return;
        }
        let first = account_arc_vec_slots(
            &mut acct.seen_ptrs,
            base,
            overlay.capacity(),
            &mut acct.package_slots,
        );
        if first {
            for p in base.iter() {
                acct.name_bytes = acct.name_bytes.saturating_add(path_bytes(p));
            }
        }
        for p in overlay.iter() {
            acct.name_bytes = acct.name_bytes.saturating_add(path_bytes(p));
        }
    }
}
