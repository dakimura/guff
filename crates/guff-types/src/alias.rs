//! Port of `cmd/compile/internal/types2/alias.go`.
//!
//! Chunk 3 ports the data and the simple constructor + accessors plus the
//! [`unalias`] chain-walker. Alias instantiation (`newAliasInstance`,
//! `subst`) is deferred to the chunk that ports the generics machinery.
//!
//! The `actual` field memoises the result of walking the alias chain to a
//! non-alias type — set by [`unalias`] on first lookup, never mutated again
//! after type-checking finishes.

use serde::{Deserialize, Serialize};

use crate::arena::{ObjectId, TypeArena, TypeData, TypeId};
use crate::typelists::{TypeList, TypeParamList};

/// An alias type.
///
/// Equivalent to `types2.Alias`. Created by alias declarations like
/// `type A = int`; the right-hand side is reachable via [`Alias::rhs`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alias {
    obj: ObjectId,                             // corresponding TypeName
    pub(crate) orig: Option<TypeId>, // None ⇒ self (uninstantiated); Some(Alias) for instances
    from_rhs: Option<TypeId>,        // RHS of the declaration; may itself be an Alias
    actual: Option<TypeId>,          // memoised non-alias result of walking the chain
    pub(crate) tparams: Option<TypeParamList>, // type parameters (None for non-generic)
    pub(crate) targs: Option<TypeList>, // type arguments (None for non-instantiated)
}

impl Alias {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.obj = r.obj(self.obj);
        self.orig = r.ty_opt(self.orig);
        self.from_rhs = r.ty_opt(self.from_rhs);
        self.actual = r.ty_opt(self.actual);
        if let Some(l) = self.tparams.as_mut() {
            l.remap_ids(r);
        }
        if let Some(l) = self.targs.as_mut() {
            l.remap_ids(r);
        }
    }
}

impl Alias {
    /// The right-hand side of the declaration `type A = R`. May itself be
    /// an alias, in which case follow [`unalias`] (or
    /// [`TypeId::underlying`](crate::TypeId::underlying)) to fully resolve.
    pub fn rhs(&self) -> Option<TypeId> {
        self.from_rhs
    }

    pub fn obj(&self) -> ObjectId {
        self.obj
    }

    pub(crate) fn set_actual(&mut self, actual: TypeId) {
        self.actual = Some(actual);
    }

    /// Type parameters of this alias type, or `None` if non-generic.
    pub fn type_params(&self) -> Option<&TypeParamList> {
        self.tparams.as_ref()
    }

    /// Type arguments used to instantiate this Alias, or `None` if not
    /// an instance.
    pub fn type_args(&self) -> Option<&TypeList> {
        self.targs.as_ref()
    }
}

/// Construct a new alias type.
///
/// Equivalent to `types2.NewAlias`. Pass `None` for `rhs` to create an
/// incomplete alias; you can fill it in later by allocating a new Alias and
/// stitching things together (chunk 3 doesn't offer a mutating `set_rhs` —
/// callers tend to construct aliases in one shot once everything's known).
///
/// If `obj`'s `typ` is `None`, it's set to the new alias (matching Go's
/// `obj.typ == nil → obj.typ = a`).
pub fn new_alias(
    arena: &mut TypeArena,
    object_arena: &mut crate::arena::ObjectArena,
    obj: ObjectId,
    rhs: Option<TypeId>,
) -> TypeId {
    let id = arena.alloc(TypeData::Alias(Alias {
        obj,
        orig: None,
        from_rhs: rhs,
        actual: None,
        tparams: None,
        targs: None,
    }));
    // Back-fill the TypeName's typ if it wasn't set.
    if obj.typ(object_arena).is_none() {
        crate::object::type_name::type_name_set_typ(object_arena, obj, id);
    }
    // Pre-compute and memoise actual — matches Go's NewAlias which calls
    // cleanup() at the tail to ensure unalias is a pure getter afterwards.
    // With no RHS yet the chain is incomplete, so memoising would cache the
    // alias itself; that alias is finished off by [`alias_set_rhs`].
    if rhs.is_some() {
        unalias(arena, id);
    }
    id
}

/// Fill in the RHS of an alias created with `rhs: None`, then re-memoise
/// `actual`.
///
/// `type A[T any] = B[T]` has to allocate the `Alias` *before* its RHS is
/// type-checked, so the type-parameter scope and the RHS can both refer to
/// `A` (Go allocates in `newAlias` and assigns `fromRHS` afterwards). This is
/// the second half of that split.
pub fn alias_set_rhs(arena: &mut TypeArena, id: TypeId, rhs: TypeId) {
    match arena.get_mut(id) {
        TypeData::Alias(a) => {
            a.from_rhs = Some(rhs);
            a.actual = None;
        }
        other => panic!("expected Alias, got {:?}", std::mem::discriminant(other)),
    }
    unalias(arena, id);
}

/// Walk the alias chain starting at `id` and return the first non-alias
/// `TypeId`. If `id` is not an alias, returns `id` unchanged. If the chain
/// is incomplete (some `fromRHS == None`), returns the last alias in the
/// chain (caller's responsibility to detect via [`TypeId::kind`]).
///
/// Memoises the result on the *first* alias of the chain via `actual`,
/// matching Go's `unalias`.
///
/// Equivalent to `types2.unalias` / `Unalias`.
pub fn unalias(arena: &mut TypeArena, id: TypeId) -> TypeId {
    // Only meaningful for Alias-rooted chains.
    let starting_is_alias = matches!(arena.get(id), TypeData::Alias(_));
    if !starting_is_alias {
        return id;
    }
    // Fast path: cached actual on the first alias.
    if let TypeData::Alias(a) = arena.get(id) {
        if let Some(actual) = a.actual {
            return actual;
        }
    }

    // Walk the chain, threading through `fromRHS` until we hit a non-alias.
    let mut current = id;
    let result = loop {
        match arena.get(current) {
            TypeData::Alias(a) => match a.from_rhs {
                Some(next) => current = next,
                None => break current, // incomplete; stop here
            },
            _ => break current,
        }
    };

    // Memoise on the starting alias.
    if let TypeData::Alias(a) = arena.get_mut(id) {
        a.set_actual(result);
    }
    result
}

/// Read-only variant of [`unalias`] — does not memoise. Useful when the
/// caller only has shared access to the arena.
pub fn unalias_readonly(arena: &TypeArena, id: TypeId) -> TypeId {
    let starting_is_alias = matches!(arena.get(id), TypeData::Alias(_));
    if !starting_is_alias {
        return id;
    }
    if let TypeData::Alias(a) = arena.get(id) {
        if let Some(actual) = a.actual {
            return actual;
        }
    }
    let mut current = id;
    let mut depth = 0u32;
    loop {
        if depth > 256 {
            return current;
        }
        depth += 1;
        match arena.get(current) {
            TypeData::Alias(a) => match a.from_rhs {
                Some(next) => current = next,
                None => return current,
            },
            _ => return current,
        }
    }
}

// Free-function accessors.

pub fn alias_obj(arena: &TypeArena, id: TypeId) -> ObjectId {
    as_alias(arena, id).obj
}

pub fn alias_rhs(arena: &TypeArena, id: TypeId) -> Option<TypeId> {
    as_alias(arena, id).from_rhs
}

/// Set the type parameters of an Alias type. Should be called immediately
/// after [`new_alias`] for generic aliases. Pass the result of
/// [`crate::typelists::bind_tparams`] (which sets each TypeParam's index).
///
/// Equivalent to `Alias.SetTypeParams`.
pub fn alias_set_type_params(arena: &mut TypeArena, id: TypeId, params: TypeParamList) {
    match arena.get_mut(id) {
        TypeData::Alias(a) => {
            assert!(
                a.targs.is_none(),
                "alias with type args cannot have type params set"
            );
            a.tparams = Some(params);
        }
        other => panic!("expected Alias, got {:?}", std::mem::discriminant(other)),
    }
}

/// Origin of an Alias — itself for declared (non-instantiated) aliases;
/// the original Alias for instantiated ones.
///
/// Equivalent to `Alias.Origin()`.
pub fn alias_origin(arena: &TypeArena, id: TypeId) -> TypeId {
    match arena.get(id) {
        TypeData::Alias(a) => a.orig.unwrap_or(id),
        other => panic!("expected Alias, got {:?}", std::mem::discriminant(other)),
    }
}

fn as_alias(arena: &TypeArena, id: TypeId) -> &Alias {
    match arena.get(id) {
        TypeData::Alias(a) => a,
        other => panic!("expected Alias, got {:?}", std::mem::discriminant(other)),
    }
}
