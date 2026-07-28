//! Port of `cmd/compile/internal/types2/unify.go`.
//!
//! Type unification: given two types `x` and `y` and a list of "tracked"
//! type parameters, attempt to find type-parameter bindings that make
//! `x` and `y` structurally equivalent. If we succeed, the inferred
//! types are recorded on the [`Unifier`].
//!
//! ## Differences from Go
//!
//! - **`handles` indirection** uses a `slot_of: HashMap<TypeId, usize>` +
//!   `handles: Vec<Option<TypeId>>` instead of Go's `map[*TypeParam]*Type`.
//!   Joining two tparams repoints all entries pointing at one of the slots
//!   to the other. Functionally equivalent.
//! - **`ifacePair`** is a `Vec<(TypeId, TypeId)>` stack (same approach as
//!   [`crate::predicates::identical`]).
//! - **TypeParam ordering for `Display`** uses TypeId arena indices, since
//!   we don't model Go's globally-monotonic `id` field.
//!
//! ## Notes / deferrals
//!
//! - **`enable_interface_inference = true`** is implemented (chunk 63): the
//!   Go-1.21+ structural interface-matching branches in `nify` (the line-338
//!   condition guard and the lines 451-545 block) are active when the flag is
//!   set. Production callers (`infer`) still pass `false` for now, so existing
//!   behaviour is unchanged; flip the flag to enable shared-method inference.
//! - **`tracef` / `String` debug output** is omitted (no `newTypeWriter`
//!   yet — lands with `typestring.go`, Tier 5).
//! - **Depth-limit panic**: matches Go's `panicAtUnificationDepthLimit =
//!   true` behaviour.

use crate::hash::HashMap;

use crate::alias::unalias_readonly;
use crate::arena::{ObjectArena, ObjectData, ObjectId, PackageArena, TypeArena, TypeData, TypeId};
use crate::chan::ChanDir;
use crate::interface::interface_compute_typeset;
use crate::lookup::{as_named, lookup_field_or_method, LookupResult};
use crate::named::named_origin;
use crate::predicates::{identical, is_interface, is_type_lit};
use crate::termlist;
use crate::under::common_under;

/// Recursion depth limit. Matches Go's `unificationDepthLimit`.
pub const UNIFICATION_DEPTH_LIMIT: u32 = 50;

/// If set, unification considers core types of non-local (unbound) type
/// parameters. Matches Go's `enableCoreTypeUnification = true`.
pub const ENABLE_CORE_TYPE_UNIFICATION: bool = true;

// ----------------------------------------------------------------------------
// UnifyMode

/// Bitset controlling [`Unifier::unify`] behaviour.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct UnifyMode(u8);

impl UnifyMode {
    pub const ZERO: UnifyMode = UnifyMode(0);
    /// `assign` — top-level inexact match, but element types must match
    /// exactly.
    pub const ASSIGN: UnifyMode = UnifyMode(1);
    /// `exact` — types must be identical (modulo tparams).
    pub const EXACT: UnifyMode = UnifyMode(2);

    pub const fn contains(self, other: UnifyMode) -> bool {
        (self.0 & other.0) == other.0
    }
    pub const fn union(self, other: UnifyMode) -> UnifyMode {
        UnifyMode(self.0 | other.0)
    }
}

// ----------------------------------------------------------------------------
// Unifier

/// Tracks unification state — the mapping from tracked TypeParams to
/// inferred types, plus the recursion-depth counter.
///
/// Equivalent to `types2.unifier`.
pub struct Unifier {
    /// For each tracked TypeParam, the slot index in `handles` that holds
    /// its (possibly None) inferred type. Joined TypeParams share a slot.
    slot_of: HashMap<TypeId, usize>,
    handles: Vec<Option<TypeId>>,
    depth: u32,
    /// Reserved for the chunk-12-deferred Go 1.21+ interface-inference
    /// path. Always `false` for chunk 12; held for forward compatibility.
    #[allow(dead_code)]
    enable_interface_inference: bool,
}

impl Unifier {
    /// Construct a new unifier tracking `tparams` with optional initial
    /// `targs`. `targs` may be shorter than `tparams`; missing entries
    /// start unset.
    ///
    /// Equivalent to `newUnifier`. When `enable_interface_inference` is `true`,
    /// the Go-1.21+ structural interface-matching branches in [`nify`] become
    /// active (chunk 63 — was the chunk-12 deferral).
    pub fn new(
        tparams: &[TypeId],
        targs: &[Option<TypeId>],
        enable_interface_inference: bool,
    ) -> Self {
        assert!(tparams.len() >= targs.len());
        let mut slot_of = HashMap::with_capacity_and_hasher(tparams.len(), Default::default());
        let mut handles = Vec::with_capacity(tparams.len());
        for (i, &tp) in tparams.iter().enumerate() {
            let initial = if i < targs.len() { targs[i] } else { None };
            handles.push(initial);
            slot_of.insert(tp, i);
        }
        Self {
            slot_of,
            handles,
            depth: 0,
            enable_interface_inference,
        }
    }

    /// Get the (possibly None) inferred type for tparam `x`.
    pub fn at(&self, x: TypeId) -> Option<TypeId> {
        let slot = *self.slot_of.get(&x).expect("not a tracked tparam");
        self.handles[slot]
    }

    /// Set the inferred type for tparam `x` (and all joined tparams).
    /// `t` must not be a TypeId that resolves back to the same tparam.
    pub fn set(&mut self, x: TypeId, t: TypeId) {
        let slot = *self.slot_of.get(&x).expect("not a tracked tparam");
        self.handles[slot] = Some(t);
    }

    /// Number of tracked tparams that still have no inferred type.
    pub fn unknowns(&self) -> usize {
        self.handles.iter().filter(|h| h.is_none()).count()
    }

    /// Return inferred types in the same order as `tparams`. Length equals
    /// `tparams.len()`. Untyped entries are `None`.
    pub fn inferred(&self, tparams: &[TypeId]) -> Vec<Option<TypeId>> {
        tparams
            .iter()
            .map(|tp| {
                let slot = *self.slot_of.get(tp).expect("not a tracked tparam");
                self.handles[slot]
            })
            .collect()
    }

    /// Reports whether `t` is a TypeParam tracked by this unifier.
    fn as_bound_type_param(&self, arena: &TypeArena, t: TypeId) -> Option<TypeId> {
        let u = unalias_readonly(arena, t);
        if matches!(arena.get(u), TypeData::TypeParam(_)) && self.slot_of.contains_key(&u) {
            Some(u)
        } else {
            None
        }
    }

    /// Join two tracked TypeParams — merge their handle slots. Returns
    /// `false` if both already have (different) inferred types.
    ///
    /// Equivalent to `unifier.join`.
    pub fn join(&mut self, x: TypeId, y: TypeId) -> bool {
        let sx = *self.slot_of.get(&x).expect("not a tracked tparam");
        let sy = *self.slot_of.get(&y).expect("not a tracked tparam");
        if sx == sy {
            return true;
        }
        let hx = self.handles[sx];
        let hy = self.handles[sy];
        match (hx, hy) {
            (Some(_), Some(_)) => false,
            (Some(t), None) => {
                self.repoint_slot(sy, sx);
                self.handles[sx] = Some(t);
                true
            }
            _ => {
                // Either hx is None or both are None — use y's slot.
                self.repoint_slot(sx, sy);
                true
            }
        }
    }

    /// Internal: redirect every tparam currently pointing at `from` to
    /// point at `to`. After this, `from`'s entry is dead.
    fn repoint_slot(&mut self, from: usize, to: usize) {
        for slot in self.slot_of.values_mut() {
            if *slot == from {
                *slot = to;
            }
        }
    }
}

// ----------------------------------------------------------------------------
// asInterface

/// Returns the underlying Interface TypeId of `x` if `x` is a non-TypeParam
/// type whose underlying is an Interface. Otherwise `None`.
///
/// Equivalent to `asInterface`.
pub fn as_interface(arena: &TypeArena, x: TypeId) -> Option<TypeId> {
    let u = unalias_readonly(arena, x);
    if matches!(arena.get(u), TypeData::TypeParam(_)) {
        return None;
    }
    let und = x.underlying(arena);
    if matches!(arena.get(und), TypeData::Interface(_)) {
        Some(und)
    } else {
        None
    }
}

// ----------------------------------------------------------------------------
// Top-level unify

/// Attempt to unify `x` and `y`. Records bindings on the unifier as a
/// side effect; returns `true` on success.
///
/// Equivalent to `unifier.unify`.
pub fn unify(
    u: &mut Unifier,
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    x: TypeId,
    y: TypeId,
    mode: UnifyMode,
) -> bool {
    let mut iface_stack: Vec<(TypeId, TypeId)> = Vec::new();
    nify(
        u,
        type_arena,
        object_arena,
        package_arena,
        x,
        y,
        mode,
        &mut iface_stack,
    )
}

// ----------------------------------------------------------------------------
// nify — the core algorithm

fn nify(
    u: &mut Unifier,
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    x: TypeId,
    y: TypeId,
    mode: UnifyMode,
    iface_stack: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    u.depth += 1;
    let result = nify_inner(
        u,
        type_arena,
        object_arena,
        package_arena,
        x,
        y,
        mode,
        iface_stack,
    );
    u.depth -= 1;
    result
}

fn nify_inner(
    u: &mut Unifier,
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    mut x: TypeId,
    mut y: TypeId,
    mode: UnifyMode,
    iface_stack: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    if x == y || unalias_readonly(type_arena, x) == unalias_readonly(type_arena, y) {
        return true;
    }
    if u.depth > UNIFICATION_DEPTH_LIMIT {
        panic!("unification reached recursion depth limit");
    }

    // Symmetry-breaking swaps:
    //  - if there's a Named, ensure it's in y
    //  - if there's a tracked TypeParam, ensure it's in x
    if as_named(type_arena, x).is_some() || u.as_bound_type_param(type_arena, y).is_some() {
        std::mem::swap(&mut x, &mut y);
    }

    // If at least one side is a Named (in y after swap), and we're in
    // inexact mode, unwrap to its underlying so a literal `x` can match.
    let exact = mode.contains(UnifyMode::EXACT);
    if let Some(ny) = as_named(type_arena, y) {
        // When interface inference is on and x is an interface, do NOT unwrap
        // the named type here — fall through to the interface-matching block
        // below instead (go.dev/issue/60564).
        if !exact
            && is_type_lit(type_arena, x)
            && !(u.enable_interface_inference && is_interface(type_arena, x))
        {
            let yu = ny.underlying(type_arena);
            y = yu;
            if x == y || unalias_readonly(type_arena, x) == unalias_readonly(type_arena, y) {
                return true;
            }
        }
    }

    // Tracked-TypeParam cases.
    let px = u.as_bound_type_param(type_arena, x);
    let py = u.as_bound_type_param(type_arena, y);
    match (px, py) {
        (Some(px), Some(py)) => {
            if u.join(px, py) {
                return true;
            }
            let ax = u.at(px);
            let ay = u.at(py);
            return match (ax, ay) {
                (Some(a), Some(b)) => nify(
                    u,
                    type_arena,
                    object_arena,
                    package_arena,
                    a,
                    b,
                    mode,
                    iface_stack,
                ),
                _ => false,
            };
        }
        (Some(px), None) => {
            if let Some(prev) = u.at(px) {
                // px already has an inferred type — must match y.
                if !nify(
                    u,
                    type_arena,
                    object_arena,
                    package_arena,
                    prev,
                    y,
                    mode,
                    iface_stack,
                ) {
                    return false;
                }
                // Defined-type preference under inexact unification.
                let xn = as_named(type_arena, prev).is_some();
                let yn = as_named(type_arena, y).is_some();
                let xi = as_interface(type_arena, prev);
                let yi = as_interface(type_arena, y);
                match (xi, yi) {
                    (Some(_), Some(_)) => {
                        if xn && yn {
                            return identical(type_arena, object_arena, package_arena, prev, y);
                        }
                        // method-set length check — both must have computed typesets
                        interface_compute_typeset(
                            type_arena,
                            object_arena,
                            package_arena,
                            xi.unwrap(),
                        );
                        interface_compute_typeset(
                            type_arena,
                            object_arena,
                            package_arena,
                            yi.unwrap(),
                        );
                        let xn_meth = match type_arena.get(xi.unwrap()) {
                            TypeData::Interface(i) => {
                                i.tset.as_ref().map(|t| t.num_methods()).unwrap_or(0)
                            }
                            _ => 0,
                        };
                        let yn_meth = match type_arena.get(yi.unwrap()) {
                            TypeData::Interface(i) => {
                                i.tset.as_ref().map(|t| t.num_methods()).unwrap_or(0)
                            }
                            _ => 0,
                        };
                        if xn_meth != yn_meth {
                            return false;
                        }
                    }
                    (Some(_), None) | (None, Some(_)) => return false,
                    _ => {}
                }
                if !exact {
                    if xn {
                        // prefer prev (already defined-type): nothing to do
                    } else if yn {
                        u.set(px, y);
                    } else {
                        // Neither is a defined type. Prefer a directed channel.
                        let yu = y.underlying(type_arena);
                        if let TypeData::Chan(c) = type_arena.get(yu) {
                            if c.dir() != ChanDir::SendRecv {
                                u.set(px, y);
                            }
                        }
                    }
                }
                return true;
            }
            // No inferred type yet — record y.
            u.set(px, y);
            return true;
        }
        _ => {}
    }

    // EnableInterfaceInference: structural interface matching (Go unify.go
    // 451-545). Active only when the flag is set and we don't require exact
    // unification. If both types are interfaces, one method set must be a
    // subset of the other and the common method signatures must unify. If only
    // one type is an interface, all its methods must be present in the other.
    if u.enable_interface_inference && !exact {
        // One or both interfaces may be defined types; look under the name but
        // not under type parameters (go.dev/issue/60564).
        let xi = as_interface(type_arena, x);
        let yi = as_interface(type_arena, y);

        if let (Some(xi), Some(yi)) = (xi, yi) {
            // Two interfaces: compare type terms for equivalence and unify the
            // common methods.
            interface_compute_typeset(type_arena, object_arena, package_arena, xi);
            interface_compute_typeset(type_arena, object_arena, package_arena, yi);
            let (xcomp, xterms, xmethods) = match type_arena.get(xi) {
                TypeData::Interface(i) => {
                    let ts = i.tset.as_ref().expect("computed above");
                    (ts.comparable(), ts.terms.clone(), ts.methods().to_vec())
                }
                _ => unreachable!(),
            };
            let (ycomp, yterms, ymethods) = match type_arena.get(yi) {
                TypeData::Interface(i) => {
                    let ts = i.tset.as_ref().expect("computed above");
                    (ts.comparable(), ts.terms.clone(), ts.methods().to_vec())
                }
                _ => unreachable!(),
            };
            if xcomp != ycomp {
                return false;
            }
            // For now we require terms to be equal (matches Go's restriction).
            if !termlist::equal(type_arena, object_arena, package_arena, &xterms, &yterms) {
                return false;
            }
            // ifacePair cycle detection: if (xi, yi) was compared before they
            // must be equal (otherwise the recursion would have stopped).
            let pair = (xi, yi);
            let pair_swapped = (yi, xi);
            if iface_stack.iter().any(|&p| p == pair || p == pair_swapped) {
                return true;
            }
            // The smaller method set must be the subset, if one exists.
            let (small, large) = if xmethods.len() > ymethods.len() {
                (ymethods, xmethods)
            } else {
                (xmethods, ymethods)
            };
            iface_stack.push(pair);
            let ok = (|| -> bool {
                for &sm in &small {
                    let sm_id = sm.id(object_arena, package_arena);
                    let lm = large
                        .iter()
                        .copied()
                        .find(|lm| lm.id(object_arena, package_arena) == sm_id);
                    let lm = match lm {
                        Some(m) => m,
                        None => return false,
                    };
                    match (sm.typ(object_arena), lm.typ(object_arena)) {
                        (Some(a), Some(b)) => {
                            if !nify(
                                u,
                                type_arena,
                                object_arena,
                                package_arena,
                                a,
                                b,
                                UnifyMode::EXACT,
                                iface_stack,
                            ) {
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
                true
            })();
            iface_stack.pop();
            return ok;
        }

        // Not two interfaces. If we have exactly one, make sure it's `xi` and
        // the non-interface side is `other`.
        let (single, other) = match (xi, yi) {
            (Some(xi), _) => (Some(xi), y),
            (None, Some(yi)) => (Some(yi), x),
            (None, None) => (None, y),
        };
        if let Some(xi) = single {
            // Each interface method must be implemented by `other` and the
            // corresponding signatures must unify.
            interface_compute_typeset(type_arena, object_arena, package_arena, xi);
            let xmethods = match type_arena.get(xi) {
                TypeData::Interface(i) => {
                    i.tset.as_ref().expect("computed above").methods().to_vec()
                }
                _ => unreachable!(),
            };
            for &xm in &xmethods {
                let pkg = xm.pkg(object_arena);
                let name = xm.name(object_arena).to_string();
                let found = lookup_field_or_method(
                    type_arena,
                    object_arena,
                    package_arena,
                    other,
                    false,
                    pkg,
                    &name,
                );
                let ym = match found {
                    LookupResult::Found { obj, .. }
                        if matches!(object_arena.get(obj), ObjectData::Func(_)) =>
                    {
                        obj
                    }
                    _ => return false,
                };
                match (xm.typ(object_arena), ym.typ(object_arena)) {
                    (Some(a), Some(b)) => {
                        if !nify(
                            u,
                            type_arena,
                            object_arena,
                            package_arena,
                            a,
                            b,
                            UnifyMode::EXACT,
                            iface_stack,
                        ) {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
            return true;
        }
    }

    // Ensure unbound TypeParam (if any) is in x.
    {
        let yu = unalias_readonly(type_arena, y);
        if matches!(type_arena.get(yu), TypeData::TypeParam(_)) {
            std::mem::swap(&mut x, &mut y);
        }
    }

    // Element mode: exact for assign-elements, otherwise inherit.
    let emode = if mode.contains(UnifyMode::ASSIGN) {
        mode.union(UnifyMode::EXACT)
    } else {
        mode
    };

    // Continue with unaliased forms (we keep `xorig` for the TypeParam case
    // where the alias name matters, like commonUnder's input).
    let xorig = x;
    let yorig = y;
    let xu = unalias_readonly(type_arena, x);
    let yu = unalias_readonly(type_arena, y);

    use crate::TypeKind as K;
    match (xu.kind(type_arena), yu.kind(type_arena)) {
        (K::Basic, K::Basic) => match (type_arena.get(xu), type_arena.get(yu)) {
            (TypeData::Basic(a), TypeData::Basic(b)) => a.kind() == b.kind(),
            _ => unreachable!(),
        },

        (K::Array, K::Array) => {
            let (xl, xe) = match type_arena.get(xu) {
                TypeData::Array(a) => (a.len(), a.elem()),
                _ => unreachable!(),
            };
            let (yl, ye) = match type_arena.get(yu) {
                TypeData::Array(a) => (a.len(), a.elem()),
                _ => unreachable!(),
            };
            (xl < 0 || yl < 0 || xl == yl)
                && nify(
                    u,
                    type_arena,
                    object_arena,
                    package_arena,
                    xe,
                    ye,
                    emode,
                    iface_stack,
                )
        }

        (K::Slice, K::Slice) => {
            let xe = match type_arena.get(xu) {
                TypeData::Slice(s) => s.elem(),
                _ => unreachable!(),
            };
            let ye = match type_arena.get(yu) {
                TypeData::Slice(s) => s.elem(),
                _ => unreachable!(),
            };
            nify(
                u,
                type_arena,
                object_arena,
                package_arena,
                xe,
                ye,
                emode,
                iface_stack,
            )
        }

        (K::Pointer, K::Pointer) => {
            let xe = match type_arena.get(xu) {
                TypeData::Pointer(p) => p.elem(),
                _ => unreachable!(),
            };
            let ye = match type_arena.get(yu) {
                TypeData::Pointer(p) => p.elem(),
                _ => unreachable!(),
            };
            nify(
                u,
                type_arena,
                object_arena,
                package_arena,
                xe,
                ye,
                emode,
                iface_stack,
            )
        }

        (K::Map, K::Map) => {
            let (xk, xe) = match type_arena.get(xu) {
                TypeData::Map(m) => (m.key(), m.elem()),
                _ => unreachable!(),
            };
            let (yk, ye) = match type_arena.get(yu) {
                TypeData::Map(m) => (m.key(), m.elem()),
                _ => unreachable!(),
            };
            nify(
                u,
                type_arena,
                object_arena,
                package_arena,
                xk,
                yk,
                emode,
                iface_stack,
            ) && nify(
                u,
                type_arena,
                object_arena,
                package_arena,
                xe,
                ye,
                emode,
                iface_stack,
            )
        }

        (K::Chan, K::Chan) => {
            let (xd, xe) = match type_arena.get(xu) {
                TypeData::Chan(c) => (c.dir(), c.elem()),
                _ => unreachable!(),
            };
            let (yd, ye) = match type_arena.get(yu) {
                TypeData::Chan(c) => (c.dir(), c.elem()),
                _ => unreachable!(),
            };
            (!exact || xd == yd)
                && nify(
                    u,
                    type_arena,
                    object_arena,
                    package_arena,
                    xe,
                    ye,
                    emode,
                    iface_stack,
                )
        }

        (K::Struct, K::Struct) => unify_structs(
            u,
            type_arena,
            object_arena,
            package_arena,
            xu,
            yu,
            emode,
            iface_stack,
        ),

        (K::Tuple, K::Tuple) => unify_tuples(
            u,
            type_arena,
            object_arena,
            package_arena,
            xu,
            yu,
            mode,
            iface_stack,
        ),

        (K::Signature, K::Signature) => unify_signatures(
            u,
            type_arena,
            object_arena,
            package_arena,
            xu,
            yu,
            emode,
            iface_stack,
        ),

        (K::Interface, K::Interface) => unify_interfaces(
            u,
            type_arena,
            object_arena,
            package_arena,
            xu,
            yu,
            iface_stack,
        ),

        (K::Named, K::Named) => unify_nameds(
            u,
            type_arena,
            object_arena,
            package_arena,
            xu,
            yu,
            mode,
            iface_stack,
        ),

        (K::TypeParam, _) => {
            // Unbound TypeParam (tracked TypeParams were handled above).
            if ENABLE_CORE_TYPE_UNIFICATION {
                let (cx, _err) = common_under(type_arena, object_arena, package_arena, xu, None);
                if let Some(cx) = cx {
                    return nify(
                        u,
                        type_arena,
                        object_arena,
                        package_arena,
                        cx,
                        yorig,
                        UnifyMode::ASSIGN,
                        iface_stack,
                    );
                }
            }
            let _ = xorig;
            false
        }

        _ => false,
    }
}

fn unify_structs(
    u: &mut Unifier,
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    x: TypeId,
    y: TypeId,
    emode: UnifyMode,
    iface_stack: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    let (xf, xt) = match type_arena.get(x) {
        TypeData::Struct(s) => (
            (0..s.num_fields()).map(|i| s.field(i)).collect::<Vec<_>>(),
            (0..s.num_fields())
                .map(|i| s.tag(i).to_string())
                .collect::<Vec<_>>(),
        ),
        _ => unreachable!(),
    };
    let (yf, yt) = match type_arena.get(y) {
        TypeData::Struct(s) => (
            (0..s.num_fields()).map(|i| s.field(i)).collect::<Vec<_>>(),
            (0..s.num_fields())
                .map(|i| s.tag(i).to_string())
                .collect::<Vec<_>>(),
        ),
        _ => unreachable!(),
    };
    if xf.len() != yf.len() {
        return false;
    }
    for i in 0..xf.len() {
        let f = xf[i];
        let g = yf[i];
        let f_embedded = matches!(object_arena.get(f), ObjectData::Var(v) if v.embedded());
        let g_embedded = matches!(object_arena.get(g), ObjectData::Var(v) if v.embedded());
        if f_embedded != g_embedded || xt[i] != yt[i] {
            return false;
        }
        let g_name = g.name(object_arena).to_string();
        let g_pkg = g.pkg(object_arena);
        if !f.same_id(object_arena, package_arena, g_pkg, &g_name, false) {
            return false;
        }
        let ftyp = f.typ(object_arena).expect("Var has typ");
        let gtyp = g.typ(object_arena).expect("Var has typ");
        if !nify(
            u,
            type_arena,
            object_arena,
            package_arena,
            ftyp,
            gtyp,
            emode,
            iface_stack,
        ) {
            return false;
        }
    }
    true
}

fn unify_tuples(
    u: &mut Unifier,
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    x: TypeId,
    y: TypeId,
    mode: UnifyMode,
    iface_stack: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    let xv: Vec<ObjectId> = match type_arena.get(x) {
        TypeData::Tuple(t) => (0..t.len()).map(|i| t.at(i)).collect(),
        _ => unreachable!(),
    };
    let yv: Vec<ObjectId> = match type_arena.get(y) {
        TypeData::Tuple(t) => (0..t.len()).map(|i| t.at(i)).collect(),
        _ => unreachable!(),
    };
    if xv.len() != yv.len() {
        return false;
    }
    for i in 0..xv.len() {
        let xt = xv[i].typ(object_arena).expect("tuple Var has typ");
        let yt = yv[i].typ(object_arena).expect("tuple Var has typ");
        if !nify(
            u,
            type_arena,
            object_arena,
            package_arena,
            xt,
            yt,
            mode,
            iface_stack,
        ) {
            return false;
        }
    }
    true
}

fn unify_signatures(
    u: &mut Unifier,
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    x: TypeId,
    y: TypeId,
    emode: UnifyMode,
    iface_stack: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    let (xv, xp, xr) = match type_arena.get(x) {
        TypeData::Signature(s) => (s.variadic(), s.params(), s.results()),
        _ => unreachable!(),
    };
    let (yv, yp, yr) = match type_arena.get(y) {
        TypeData::Signature(s) => (s.variadic(), s.params(), s.results()),
        _ => unreachable!(),
    };
    if xv != yv {
        return false;
    }
    if !nify_optional_tuple(
        u,
        type_arena,
        object_arena,
        package_arena,
        xp,
        yp,
        emode,
        iface_stack,
    ) {
        return false;
    }
    nify_optional_tuple(
        u,
        type_arena,
        object_arena,
        package_arena,
        xr,
        yr,
        emode,
        iface_stack,
    )
}

/// Helper: unify two Optional Tuples — `None` means the empty tuple.
fn nify_optional_tuple(
    u: &mut Unifier,
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    x: Option<TypeId>,
    y: Option<TypeId>,
    mode: UnifyMode,
    iface_stack: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    match (x, y) {
        (None, None) => true,
        (Some(a), Some(b)) => nify(
            u,
            type_arena,
            object_arena,
            package_arena,
            a,
            b,
            mode,
            iface_stack,
        ),
        _ => false,
    }
}

fn unify_interfaces(
    u: &mut Unifier,
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    x: TypeId,
    y: TypeId,
    iface_stack: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    // Compute typesets for both.
    interface_compute_typeset(type_arena, object_arena, package_arena, x);
    interface_compute_typeset(type_arena, object_arena, package_arena, y);
    let (xcomp, xterms, xmethods) = match type_arena.get(x) {
        TypeData::Interface(i) => {
            let ts = i.tset.as_ref().expect("computed above");
            (ts.comparable(), ts.terms.clone(), ts.methods().to_vec())
        }
        _ => unreachable!(),
    };
    let (ycomp, yterms, ymethods) = match type_arena.get(y) {
        TypeData::Interface(i) => {
            let ts = i.tset.as_ref().expect("computed above");
            (ts.comparable(), ts.terms.clone(), ts.methods().to_vec())
        }
        _ => unreachable!(),
    };
    if xcomp != ycomp {
        return false;
    }
    if !termlist::equal(type_arena, object_arena, package_arena, &xterms, &yterms) {
        return false;
    }
    if xmethods.len() != ymethods.len() {
        return false;
    }
    // ifacePair cycle detection.
    let pair = (x, y);
    let pair_swapped = (y, x);
    if iface_stack.iter().any(|&p| p == pair || p == pair_swapped) {
        return true;
    }
    iface_stack.push(pair);
    let ok = (|stack: &mut Vec<(TypeId, TypeId)>| -> bool {
        for i in 0..xmethods.len() {
            let xm = xmethods[i];
            let ym = ymethods[i];
            if xm.id(object_arena, package_arena) != ym.id(object_arena, package_arena) {
                return false;
            }
            let xt = xm.typ(object_arena);
            let yt = ym.typ(object_arena);
            match (xt, yt) {
                (Some(a), Some(b)) => {
                    if !nify(
                        u,
                        type_arena,
                        object_arena,
                        package_arena,
                        a,
                        b,
                        UnifyMode::EXACT,
                        stack,
                    ) {
                        return false;
                    }
                }
                (None, None) => {}
                _ => return false,
            }
        }
        true
    })(iface_stack);
    iface_stack.pop();
    ok
}

fn unify_nameds(
    u: &mut Unifier,
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    x: TypeId,
    y: TypeId,
    mode: UnifyMode,
    iface_stack: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    let xargs: Vec<TypeId> = crate::named::named_type_args(type_arena, x)
        .map(|l| l.list().to_vec())
        .unwrap_or_default();
    let yargs: Vec<TypeId> = crate::named::named_type_args(type_arena, y)
        .map(|l| l.list().to_vec())
        .unwrap_or_default();
    if xargs.len() != yargs.len() {
        return false;
    }
    for i in 0..xargs.len() {
        if !nify(
            u,
            type_arena,
            object_arena,
            package_arena,
            xargs[i],
            yargs[i],
            mode,
            iface_stack,
        ) {
            return false;
        }
    }
    // Origin equality via TypeName ObjectId.
    let xo = named_origin(type_arena, x);
    let yo = named_origin(type_arena, y);
    let xobj = match type_arena.get(xo) {
        TypeData::Named(n) => n.obj(),
        _ => return false,
    };
    let yobj = match type_arena.get(yo) {
        TypeData::Named(n) => n.obj(),
        _ => return false,
    };
    xobj == yobj
}
