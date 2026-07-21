//! Port of `cmd/compile/internal/types2/typeparam.go`.
//!
//! Chunk 3 ports the data + simple accessors. The `iface()` machinery —
//! wrapping non-interface bounds in an implicit interface, computing the
//! type set — depends on `typeset.go` and lands in a later chunk. Until
//! then, [`TypeId::underlying`](crate::TypeId::underlying) returns the
//! constraint's underlying directly if it's already an Interface, and `self`
//! otherwise (with a TODO).
//!
//! The `id` field (a globally-monotonic counter used by Go for debug output)
//! is omitted — not load-bearing.

use crate::arena::{ObjectArena, ObjectId, TypeArena, TypeData, TypeId};

/// A type parameter in a generic declaration.
///
/// Equivalent to `types2.TypeParam`. `index` is `-1` until the param is
/// bound to a type via [`crate::typelists::bind_tparams`].
#[derive(Debug, Clone)]
pub struct TypeParam {
    obj: ObjectId,         // corresponding TypeName
    index: i32,            // -1 until bound
    bound: Option<TypeId>, // any type; chunk 3 expects Interface or None
}

impl TypeParam {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.obj = r.obj(self.obj);
        self.bound = r.ty_opt(self.bound);
    }
}

impl TypeParam {
    /// The type name for this type parameter.
    pub fn obj(&self) -> ObjectId {
        self.obj
    }

    /// Index of this param within its param list. `-1` if not yet bound.
    pub fn index(&self) -> i32 {
        self.index
    }

    /// The type constraint specified for this parameter, or `None` if it
    /// hasn't been set yet (matches Go's `NewTypeParam(.., nil)` path).
    pub fn constraint(&self) -> Option<TypeId> {
        self.bound
    }

    pub(crate) fn set_index(&mut self, index: i32) {
        self.index = index;
    }

    pub(crate) fn set_constraint(&mut self, bound: TypeId) {
        self.bound = Some(bound);
    }
}

/// Construct a new [`TypeParam`].
///
/// Equivalent to `types2.NewTypeParam`. The `constraint` can be `None` and
/// set later via [`set_constraint`]. If the corresponding [`TypeName`] has
/// `typ` unset, the caller should follow up with
/// [`crate::object::type_name::type_name_set_typ`] to wire the binding back.
///
/// [`TypeName`]: crate::object::type_name::TypeName
pub fn new_type_param(arena: &mut TypeArena, obj: ObjectId, constraint: Option<TypeId>) -> TypeId {
    arena.alloc(TypeData::TypeParam(TypeParam {
        obj,
        index: -1,
        bound: constraint,
    }))
}

/// Set the constraint of an existing type parameter.
///
/// Equivalent to `types2.TypeParam.SetConstraint`. Unlike Go, we don't
/// re-run `iface()` to mutate-and-validate the bound — that happens with
/// typeset.go in a later chunk.
///
/// # Panics
/// Panics if `id` does not refer to a `TypeParam`.
pub fn set_constraint(arena: &mut TypeArena, id: TypeId, bound: TypeId) {
    match arena.get_mut(id) {
        TypeData::TypeParam(tp) => tp.set_constraint(bound),
        other => panic!(
            "expected TypeParam, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

// Free-function accessors.

pub fn type_param_obj(arena: &TypeArena, id: TypeId) -> ObjectId {
    as_type_param(arena, id).obj
}

pub fn type_param_index(arena: &TypeArena, id: TypeId) -> i32 {
    as_type_param(arena, id).index
}

pub fn type_param_constraint(arena: &TypeArena, id: TypeId) -> Option<TypeId> {
    as_type_param(arena, id).bound
}

fn as_type_param(arena: &TypeArena, id: TypeId) -> &TypeParam {
    match arena.get(id) {
        TypeData::TypeParam(tp) => tp,
        other => panic!(
            "expected TypeParam, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

// ----------------------------------------------------------------------------
// iface() — the full constraint-interface resolution (chunk 4)

/// Returns the constraint interface of this type parameter.
///
/// Equivalent to `types2.TypeParam.iface()`. The chunk-4 semantics:
///
/// - If the bound is `None`, returns a freshly-allocated empty interface.
/// - If the bound's underlying is `Invalid` Basic, returns an empty
///   interface.
/// - If the bound's underlying is already an `Interface`, returns that
///   interface's `TypeId` (and computes its type set).
/// - Otherwise wraps the bound in an implicit Interface
///   (`NewInterfaceType(nil, [bound])`), mutates the TypeParam's bound to
///   point at the wrapper, and returns the wrapper.
///
/// The returned Interface always has its type set computed.
///
/// # Panics
/// Panics if `id` does not refer to a `TypeParam`.
pub fn type_param_iface(
    arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &crate::arena::PackageArena,
    id: TypeId,
) -> TypeId {
    let bound = match arena.get(id) {
        TypeData::TypeParam(tp) => tp.bound,
        other => panic!(
            "type_param_iface: expected TypeParam, got {:?}",
            std::mem::discriminant(other)
        ),
    };

    // No bound → empty interface.
    let bound = match bound {
        Some(b) => b,
        None => {
            let empty = crate::interface::new_interface_type(arena, vec![], vec![]);
            crate::interface::interface_compute_typeset(arena, object_arena, package_arena, empty);
            return empty;
        }
    };

    // Underlying of bound — if Interface, use it directly. If Invalid Basic,
    // fall back to empty.
    let u = bound.underlying(arena);
    let kind = u.kind(arena);
    match kind {
        crate::TypeKind::Interface => {
            // Already an Interface — compute typeset and return.
            crate::interface::interface_compute_typeset(arena, object_arena, package_arena, u);
            u
        }
        crate::TypeKind::Basic => {
            // Check if it's the Invalid kind.
            let is_invalid = matches!(
                arena.get(u),
                TypeData::Basic(b) if b.kind() == crate::basic::BasicKind::Invalid
            );
            if is_invalid {
                let empty = crate::interface::new_interface_type(arena, vec![], vec![]);
                crate::interface::interface_compute_typeset(
                    arena,
                    object_arena,
                    package_arena,
                    empty,
                );
                empty
            } else {
                // Non-Invalid basic — wrap.
                wrap_in_implicit_interface(arena, object_arena, package_arena, id, bound)
            }
        }
        _ => wrap_in_implicit_interface(arena, object_arena, package_arena, id, bound),
    }
}

/// Wrap `bound` in an implicit Interface, update the TypeParam's bound to
/// point at the wrapper, and return the wrapper TypeId.
fn wrap_in_implicit_interface(
    arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &crate::arena::PackageArena,
    tp_id: TypeId,
    bound: TypeId,
) -> TypeId {
    let wrapper = crate::interface::new_interface_type(arena, vec![], vec![bound]);
    crate::interface::interface_mark_implicit(arena, wrapper);
    crate::interface::interface_compute_typeset(arena, object_arena, package_arena, wrapper);

    // Update TypeParam.bound to the wrapper (Go's optimisation).
    match arena.get_mut(tp_id) {
        TypeData::TypeParam(tp) => tp.bound = Some(wrapper),
        _ => unreachable!(),
    }
    wrapper
}

/// Full underlying for a TypeParam — returns the constraint interface
/// produced by [`type_param_iface`]. Use this when you have `&mut`
/// access; the read-only [`TypeId::underlying`](crate::TypeId::underlying)
/// returns `self` for cases that would require wrapping.
pub fn type_param_underlying_full(
    arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &crate::arena::PackageArena,
    id: TypeId,
) -> TypeId {
    type_param_iface(arena, object_arena, package_arena, id)
}
