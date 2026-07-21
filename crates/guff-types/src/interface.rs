//! Port of `cmd/compile/internal/types2/interface.go`.
//!
//! Chunk 2 ported the data portion plus the "explicit" accessors. Chunk 4
//! adds the lazy `tset` cache and the typeset-derived accessors
//! ([`interface_num_methods`], [`interface_method`], [`interface_empty`],
//! [`interface_is_method_set`], [`interface_is_comparable`],
//! [`interface_typeset`]).

use crate::arena::{ObjectArena, ObjectId, PackageArena, TypeArena, TypeData, TypeId};
use crate::typeset::{compute_interface_type_set, TypeSet};

/// An interface type.
///
/// Equivalent to `types2.Interface`. The lazily-computed type set (`tset`),
/// embedded-element positions (`embedPos`), and `check` back-pointer are
/// omitted until the typeset/Checker logic lands; chunk-2 callers can build
/// and inspect the explicit-method/embedded structure.
#[derive(Debug, Clone)]
pub struct Interface {
    pub(crate) methods: Vec<ObjectId>, // ordered explicitly-declared methods (Func objects)
    pub(crate) embeddeds: Vec<TypeId>, // ordered explicitly-embedded elements
    implicit: bool,                    // wrapper for `~T`, `A|B`, or non-interface T
    pub(crate) complete: bool,         // all fields except tset are set up
    /// Lazily-computed by [`compute_interface_type_set`]. Public so the
    /// typeset module can populate it.
    pub(crate) tset: Option<TypeSet>,
}

impl Interface {
    /// Relocate ids when merging into a shared seed base (R25). Includes the
    /// lazily-computed `tset` cache, whose ids must move with everything else.
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        for m in &mut self.methods {
            *m = r.obj(*m);
        }
        for e in &mut self.embeddeds {
            *e = r.ty(*e);
        }
        if let Some(ts) = self.tset.as_mut() {
            ts.remap_ids(r);
        }
    }
}

impl Interface {
    /// Number of explicitly declared methods. Does **not** include methods
    /// gained via embedding — use [`interface_num_methods`] for that.
    pub fn num_explicit_methods(&self) -> usize {
        self.methods.len()
    }

    /// The `i`'th explicitly declared method; ordered by unique Id (Go's
    /// `sortMethods`). The sort is the caller's responsibility in chunk 2.
    pub fn explicit_method(&self, i: usize) -> ObjectId {
        self.methods[i]
    }

    /// Number of embedded types.
    pub fn num_embeddeds(&self) -> usize {
        self.embeddeds.len()
    }

    /// The `i`'th embedded type.
    pub fn embedded_type(&self, i: usize) -> TypeId {
        self.embeddeds[i]
    }

    /// Reports whether this interface is a wrapper for a type set literal
    /// (such as `~T` or `A|B`) rather than a user-written `interface{...}`.
    pub fn is_implicit(&self) -> bool {
        self.implicit
    }

    /// Mark the interface as implicit. Must be called before any concurrent
    /// use of implicit interfaces (matching Go's documented requirement).
    pub fn mark_implicit(&mut self) {
        self.implicit = true;
    }

    /// Reports whether all non-tset fields are set up.
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Read-only access to the cached type set. `None` if it hasn't been
    /// computed yet — use [`interface_typeset`] for lazy access.
    pub fn cached_typeset(&self) -> Option<&TypeSet> {
        self.tset.as_ref()
    }
}

/// Construct a new interface type with the given explicit methods and
/// embedded types.
///
/// Equivalent to `types2.NewInterfaceType` minus chunk-2-deferred bits:
/// - **No method-receiver back-fill.** Go's `NewInterfaceType` sets each
///   method's receiver to a newly-allocated `Var` pointing at the interface;
///   we skip that — chunk 2 callers can leave method receivers as `None`
///   and they'll be wired up when the Checker / scope infrastructure lands.
/// - **No method sorting.** Caller is responsible for sorting `methods` by
///   their unique Id (currently we don't have `Id()`).
pub fn new_interface_type(
    arena: &mut TypeArena,
    methods: Vec<ObjectId>,
    embeddeds: Vec<TypeId>,
) -> TypeId {
    arena.alloc(TypeData::Interface(Interface {
        methods,
        embeddeds,
        implicit: false,
        complete: true,
        tset: None,
    }))
}

// Free-function accessors.

pub fn interface_num_explicit_methods(arena: &TypeArena, id: TypeId) -> usize {
    as_interface(arena, id).num_explicit_methods()
}

pub fn interface_explicit_method(arena: &TypeArena, id: TypeId, i: usize) -> ObjectId {
    as_interface(arena, id).explicit_method(i)
}

pub fn interface_num_embeddeds(arena: &TypeArena, id: TypeId) -> usize {
    as_interface(arena, id).num_embeddeds()
}

pub fn interface_embedded_type(arena: &TypeArena, id: TypeId, i: usize) -> TypeId {
    as_interface(arena, id).embedded_type(i)
}

pub fn interface_is_implicit(arena: &TypeArena, id: TypeId) -> bool {
    as_interface(arena, id).is_implicit()
}

/// Mutating accessor — sets the implicit flag on the interface.
pub fn interface_mark_implicit(arena: &mut TypeArena, id: TypeId) {
    match arena.get_mut(id) {
        TypeData::Interface(i) => i.mark_implicit(),
        other => panic!(
            "expected Interface, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

// ----------------------------------------------------------------------------
// Typeset-derived accessors (chunk 4)
//
// All of these compute the lazy `tset` cache on first call. They take
// `&mut TypeArena` for that reason; callers that want strictly-read access
// can call [`interface_compute_typeset`] first and then use the
// [`Interface::cached_typeset`] view.

/// Force computation of the interface's type set, populating the
/// [`Interface::cached_typeset`] cache. No-op if already cached.
pub fn interface_compute_typeset(
    arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    id: TypeId,
) {
    compute_interface_type_set(arena, object_arena, package_arena, id);
}

/// Lazily compute (if needed) and return a snapshot of the type set.
///
/// Returns an owned `TypeSet` for borrow-checker ergonomics — the cache is
/// kept inside the arena, but we hand back a clone so callers don't need to
/// keep a borrow into the arena across other arena operations.
pub fn interface_typeset(
    arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    id: TypeId,
) -> TypeSet {
    compute_interface_type_set(arena, object_arena, package_arena, id);
    match arena.get(id) {
        TypeData::Interface(i) => i
            .tset
            .clone()
            .expect("compute_interface_type_set must populate tset"),
        other => panic!(
            "expected Interface, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

/// Total number of methods (explicit + via embedded interfaces).
///
/// Equivalent to `Interface.NumMethods`.
pub fn interface_num_methods(
    arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    id: TypeId,
) -> usize {
    compute_interface_type_set(arena, object_arena, package_arena, id);
    match arena.get(id) {
        TypeData::Interface(i) => i.tset.as_ref().unwrap().num_methods(),
        _ => unreachable!(),
    }
}

/// The `i`'th method (from the full type set), ordered by `Object.cmp`.
///
/// Equivalent to `Interface.Method`.
pub fn interface_method(
    arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    id: TypeId,
    i: usize,
) -> ObjectId {
    compute_interface_type_set(arena, object_arena, package_arena, id);
    match arena.get(id) {
        TypeData::Interface(iface) => iface.tset.as_ref().unwrap().method(i),
        _ => unreachable!(),
    }
}

/// Reports whether this interface is the empty interface (the set of all
/// types, with no methods).
///
/// Equivalent to `Interface.Empty`.
pub fn interface_empty(
    arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    id: TypeId,
) -> bool {
    compute_interface_type_set(arena, object_arena, package_arena, id);
    match arena.get(id) {
        TypeData::Interface(i) => i.tset.as_ref().unwrap().is_all(),
        _ => unreachable!(),
    }
}

/// Reports whether this interface is fully described by its method set
/// (no `~T` / union restrictions).
///
/// Equivalent to `Interface.IsMethodSet`.
pub fn interface_is_method_set(
    arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    id: TypeId,
) -> bool {
    compute_interface_type_set(arena, object_arena, package_arena, id);
    match arena.get(id) {
        TypeData::Interface(i) => i.tset.as_ref().unwrap().is_method_set(),
        _ => unreachable!(),
    }
}

/// Reports whether each type in this interface's type set is comparable.
///
/// Equivalent to `Interface.IsComparable`. The proper test requires
/// `comparableType` from predicates.go to walk the term list; we
/// conservatively report true only when the explicit `comparable` flag is
/// set on the type set.
pub fn interface_is_comparable(
    arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    id: TypeId,
) -> bool {
    compute_interface_type_set(arena, object_arena, package_arena, id);
    match arena.get(id) {
        TypeData::Interface(i) => {
            let ts = i.tset.as_ref().unwrap();
            ts.comparable()
        }
        _ => unreachable!(),
    }
}

/// Force the type set onto the interface and mark it as comparable.
///
/// Equivalent to Go's `&Interface{tset: &_TypeSet{nil, allTermlist, true}}`
/// trick used for the predeclared `comparable` interface. Useful as a
/// building block for [`crate::universe`].
pub fn interface_set_comparable(
    arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    id: TypeId,
) {
    interface_compute_typeset(arena, object_arena, package_arena, id);
    match arena.get_mut(id) {
        TypeData::Interface(i) => {
            if let Some(ts) = i.tset.as_mut() {
                ts.set_comparable(true);
            }
        }
        other => panic!(
            "expected Interface, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

fn as_interface(arena: &TypeArena, id: TypeId) -> &Interface {
    match arena.get(id) {
        TypeData::Interface(i) => i,
        other => panic!(
            "expected Interface, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}
