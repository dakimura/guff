//! Port of `cmd/compile/internal/types2/named.go`.
//!
//! Chunk 3 ports the data structure and the core lifecycle —
//! [`new_named`] (with optional underlying for two-phase construction),
//! [`set_underlying`], [`add_method`], and the accessors. **Deferred** to
//! later chunks:
//!
//! - **Lazy loader** (`loader func(*Named) ([]*TypeParam, Type, []*Func,
//!   []func())`) — meaningful only when loading from export data; not
//!   used in pure type-checking.
//! - **Instantiated types** (`inst *instance`) — generics instantiation
//!   needs `subst.go` / `unify.go`, lands with the generics chunk.
//! - **State machine + mutex** — Go's `lazyLoaded` / `unpacked` /
//!   `hasMethods` / `hasUnder` / `hasFinite` bits guard concurrent type
//!   checking; we run serially.
//! - **`resolveUnderlying`** chain-walking — only matters during
//!   type-checking when a Named's `fromRHS` may forward to another Named
//!   without `underlying` being cached. For chunk 3, callers wire
//!   `underlying` explicitly via [`new_named`] (eager) or
//!   [`set_underlying`].

use serde::{Deserialize, Serialize};

use crate::arena::{ObjectId, TypeArena, TypeData, TypeId};
use crate::typelists::{TypeList, TypeParamList};

/// Information specific to instantiated Named types.
///
/// Equivalent to `types2.instance`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub orig: TypeId,    // original, uninstantiated Named
    pub targs: TypeList, // type arguments
}

/// A named (defined) type.
///
/// Equivalent to `types2.Named`. Created by declarations like
/// `type S struct { ... }`; bound to a [`TypeName`](crate::object::type_name::TypeName)
/// via [`obj`](Named::obj).
///
/// The `allow_nil_*` flags mirror Go's escape hatches for the brief window
/// between `new_named(obj, None, None)` and `set_underlying`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Named {
    obj: ObjectId,
    from_rhs: Option<TypeId>,
    underlying: Option<TypeId>,
    methods: Vec<ObjectId>, // Func objects
    allow_nil_rhs: bool,
    allow_nil_underlying: bool,
    /// Type parameters of this Named (Go: `Named.tparams`). `None` for
    /// non-generic Named types.
    pub(crate) tparams: Option<TypeParamList>,
    /// Set when this Named is an instantiation. `None` for declared
    /// (origin) Named types.
    pub(crate) inst: Option<Instance>,
}

impl Named {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.obj = r.obj(self.obj);
        self.from_rhs = r.ty_opt(self.from_rhs);
        self.underlying = r.ty_opt(self.underlying);
        for m in &mut self.methods {
            *m = r.obj(*m);
        }
        if let Some(l) = self.tparams.as_mut() {
            l.remap_ids(r);
        }
        if let Some(inst) = self.inst.as_mut() {
            inst.orig = r.ty(inst.orig);
            inst.targs.remap_ids(r);
        }
    }
}

impl Named {
    pub fn obj(&self) -> ObjectId {
        self.obj
    }

    /// The declaration RHS this named type is derived from. May be `None`
    /// during two-phase construction.
    pub fn from_rhs(&self) -> Option<TypeId> {
        self.from_rhs
    }

    /// The cached underlying type. `None` for incomplete Nameds; once
    /// [`set_underlying`] runs, this is always `Some`.
    ///
    /// Note: in Go, `Named.Underlying()` may *compute* the underlying by
    /// walking forwarding-declaration chains; we defer that to the
    /// Checker-bearing chunk and only return the cached value here. The
    /// crate-level [`TypeId::underlying`](crate::TypeId::underlying)
    /// returns `self` for incomplete Nameds (matching Go's nil-returning
    /// behaviour for "no underlying yet").
    pub fn underlying(&self) -> Option<TypeId> {
        self.underlying
    }

    pub fn num_methods(&self) -> usize {
        self.methods.len()
    }

    pub fn method(&self, i: usize) -> ObjectId {
        self.methods[i]
    }

    pub fn allow_nil_rhs(&self) -> bool {
        self.allow_nil_rhs
    }

    pub fn allow_nil_underlying(&self) -> bool {
        self.allow_nil_underlying
    }

    /// Type parameters of this named type, or `None` if non-generic.
    pub fn type_params(&self) -> Option<&TypeParamList> {
        self.tparams.as_ref()
    }

    /// Returns the instance metadata if this Named is an instantiation,
    /// else `None`.
    pub fn instance(&self) -> Option<&Instance> {
        self.inst.as_ref()
    }

    /// Reports whether this Named is an instantiation.
    pub fn is_instance(&self) -> bool {
        self.inst.is_some()
    }

    /// Mark this Named invalid by overwriting its RHS and underlying with
    /// `invalid` (`Typ[Invalid]`). Used by [`crate::validtype::valid_type`]
    /// when a recursive cycle is detected.
    pub(crate) fn invalidate(&mut self, invalid: TypeId) {
        self.from_rhs = Some(invalid);
        self.underlying = Some(invalid);
    }
}

/// Construct a new named type.
///
/// Equivalent to `types2.NewNamed`. If `underlying` is `Some`, it's set
/// immediately and the `allow_nil_*` flags stay false. If `underlying` is
/// `None`, the named type is created in an "incomplete" state — callers
/// must follow up with [`set_underlying`] before the type is safe to
/// inspect via `.underlying()` / [`TypeId::underlying`].
///
/// If `obj`'s `typ` is `None`, it's set to the new named type — matches
/// Go's "if obj.typ == nil { obj.typ = n }" behaviour.
///
/// # Panics
/// Panics if `underlying` itself refers to a `Named` (matches Go's
/// `panic("underlying type must not be *Named")`).
pub fn new_named(
    type_arena: &mut TypeArena,
    object_arena: &mut crate::arena::ObjectArena,
    obj: ObjectId,
    underlying: Option<TypeId>,
    methods: Vec<ObjectId>,
) -> TypeId {
    // Guard: underlying must not itself be a Named (Go's invariant).
    if let Some(u) = underlying {
        if matches!(type_arena.get(u), TypeData::Named(_)) {
            panic!("underlying type must not be *Named");
        }
    }

    let (allow_nil_rhs, allow_nil_underlying) = (underlying.is_none(), underlying.is_none());
    let id = type_arena.alloc(TypeData::Named(Box::new(Named {
        obj,
        from_rhs: underlying,
        underlying,
        methods,
        allow_nil_rhs,
        allow_nil_underlying,
        tparams: None,
        inst: None,
    })));

    // Back-fill TypeName.typ if it wasn't set.
    if obj.typ(object_arena).is_none() {
        crate::object::type_name::type_name_set_typ(object_arena, obj, id);
    }
    id
}

/// Set the underlying type and mark the named type complete.
///
/// Equivalent to `types2.Named.SetUnderlying`. The underlying must not be
/// another `Named` (or `None`).
///
/// # Panics
/// - If `id` is not a `Named`.
/// - If `u` is `None` or refers to another `Named` (Go's invariants).
pub fn set_underlying(arena: &mut TypeArena, id: TypeId, u: TypeId) {
    if matches!(arena.get(u), TypeData::Named(_)) {
        panic!("underlying type must not be *Named");
    }
    match arena.get_mut(id) {
        TypeData::Named(n) => {
            n.from_rhs = Some(u);
            n.allow_nil_rhs = false;
            n.underlying = Some(u);
            n.allow_nil_underlying = false;
        }
        other => panic!("expected Named, got {:?}", std::mem::discriminant(other)),
    }
}

/// Set the type parameters of a Named type. Should be called immediately
/// after [`new_named`] for generic types. The TypeParam IDs in `params`
/// must each already have their `index` set (e.g. via
/// [`crate::typelists::bind_tparams`]) before being passed here — or
/// just pass the result of `bind_tparams` itself.
pub fn named_set_type_params(arena: &mut TypeArena, id: TypeId, params: TypeParamList) {
    match arena.get_mut(id) {
        TypeData::Named(n) => n.tparams = Some(params),
        other => panic!("expected Named, got {:?}", std::mem::discriminant(other)),
    }
}

/// Origin of a Named type — itself for declared (non-instantiated)
/// types; the original Named for instantiated ones.
///
/// Equivalent to `Named.Origin()`.
pub fn named_origin(arena: &TypeArena, id: TypeId) -> TypeId {
    match arena.get(id) {
        TypeData::Named(n) => n.inst.as_ref().map_or(id, |i| i.orig),
        other => panic!("expected Named, got {:?}", std::mem::discriminant(other)),
    }
}

/// Type arguments used to instantiate this Named, or `None` for declared
/// (non-instantiated) types.
///
/// Equivalent to `Named.TypeArgs()`.
pub fn named_type_args<'a>(arena: &'a TypeArena, id: TypeId) -> Option<&'a TypeList> {
    match arena.get(id) {
        TypeData::Named(n) => n.inst.as_ref().map(|i| &i.targs),
        _ => None,
    }
}

/// Add a method to the named type unless a method with the same name is
/// already present. Returns `true` if the method was added.
///
/// Equivalent to `types2.Named.AddMethod` (the same-package / no-instance
/// asserts are deferred until Package and instances land — caller's
/// responsibility for now).
///
/// # Panics
/// Panics if `id` is not a `Named`, or `m` is not a `Func` object.
/// Append a method to `id`'s method list without deduplication. Used to expand
/// a generic instance's methods from its origin (indices must correspond to the
/// origin's list, so no dedup / reordering is applied).
pub fn push_method(arena: &mut TypeArena, id: TypeId, m: ObjectId) {
    match arena.get_mut(id) {
        TypeData::Named(n) => n.methods.push(m),
        other => panic!(
            "push_method: expected Named, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

pub fn add_method(
    arena: &mut TypeArena,
    object_arena: &crate::arena::ObjectArena,
    id: TypeId,
    m: ObjectId,
) -> bool {
    // Assert `m` is a Func and grab its name (read-only).
    let m_name = match object_arena.get(m) {
        crate::arena::ObjectData::Func(f) => f.name().to_string(),
        other => panic!(
            "add_method: expected Func, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    if m_name == "_" {
        // Blank methods are not deduplicated but also not searched for;
        // Go's `methodIndex` returns -1 for "_". We simply append.
        match arena.get_mut(id) {
            TypeData::Named(n) => n.methods.push(m),
            other => panic!("expected Named, got {:?}", std::mem::discriminant(other)),
        }
        return true;
    }
    match arena.get_mut(id) {
        TypeData::Named(n) => {
            let already = n.methods.iter().any(|existing| {
                matches!(object_arena.get(*existing),
                    crate::arena::ObjectData::Func(f) if f.name() == m_name)
            });
            if already {
                return false;
            }
            n.methods.push(m);
            true
        }
        other => panic!("expected Named, got {:?}", std::mem::discriminant(other)),
    }
}

/// Search for a method by name on the Named type. Gates the search by Go's
/// "different identifier" rule (unexported names in a different package
/// don't match unless `fold_case` is true).
///
/// Returns `(index, ObjectId)` of the matching method, or `None` if no
/// match. The `pkg` argument is the package of the caller (typically the
/// package containing the selector expression `x.f`); pass `None` for
/// universe-package lookups.
///
/// Equivalent to `types2.Named.lookupMethod` (without the
/// instance-method-expansion path, which is the chunk-9 deferral).
pub fn named_lookup_method(
    type_arena: &TypeArena,
    object_arena: &crate::arena::ObjectArena,
    package_arena: &crate::arena::PackageArena,
    id: TypeId,
    pkg: Option<crate::arena::PackageId>,
    name: &str,
    fold_case: bool,
) -> Option<(usize, ObjectId)> {
    if name == "_" {
        return None;
    }
    let n = as_named(type_arena, id);
    let obj_pkg = n.obj.pkg(object_arena);
    let same_pkg = match (obj_pkg, pkg) {
        (None, None) => true,
        (Some(a), Some(b)) => a == b || package_arena.get(a).path() == package_arena.get(b).path(),
        _ => false,
    };
    let exported = crate::object::is_exported(name);
    if !(same_pkg || exported || fold_case) {
        return None;
    }

    // For a generic *instance* (`Box[int]`), the method list is not stored on
    // the instance itself (it would require eagerly expanding every method's
    // signature); it is derived from the origin, with matching indices. Search
    // the origin's methods so `Box[int].Get` resolves to `Box.Get`; the caller
    // substitutes the instance's type arguments into the returned method's
    // signature (see `Checker::method_sig_for_recv`). Mirrors the fact that
    // Go's `Named.Method(i)` on an instance returns an expanded copy of the
    // origin's i-th method.
    let methods_owner = match n.instance() {
        Some(inst) if n.methods.is_empty() => inst.orig,
        _ => id,
    };
    let sn = as_named(type_arena, methods_owner);

    if fold_case {
        for (i, m) in sn.methods.iter().enumerate() {
            if m.name(object_arena).eq_ignore_ascii_case(name) {
                return Some((i, *m));
            }
        }
    } else {
        for (i, m) in sn.methods.iter().enumerate() {
            if m.name(object_arena) == name {
                return Some((i, *m));
            }
        }
    }
    None
}

// Free-function accessors.

pub fn named_obj(arena: &TypeArena, id: TypeId) -> ObjectId {
    as_named(arena, id).obj
}

pub fn named_underlying(arena: &TypeArena, id: TypeId) -> Option<TypeId> {
    as_named(arena, id).underlying
}

pub fn named_num_methods(arena: &TypeArena, id: TypeId) -> usize {
    as_named(arena, id).num_methods()
}

pub fn named_method(arena: &TypeArena, id: TypeId, i: usize) -> ObjectId {
    as_named(arena, id).method(i)
}

fn as_named(arena: &TypeArena, id: TypeId) -> &Named {
    match arena.get(id) {
        TypeData::Named(n) => n,
        other => panic!("expected Named, got {:?}", std::mem::discriminant(other)),
    }
}
