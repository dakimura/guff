//! Port of `cmd/compile/internal/types2/predicates.go`.
//!
//! Type predicates and the structural [`identical`] equality check, plus
//! [`comparable`] / [`comparable_type`], [`default_type`], [`max_type`], and
//! [`is_valid_name`].
//!
//! Chunk-5 deferrals (documented inline at the call site):
//! - `Signature` identical (chunk 70): generic signatures are identical
//!   modulo type-parameter renaming. Instead of materialising a substituted
//!   `y` via `subst.go` (which needs `&mut ObjectArena`/`Context`), we thread
//!   a `y-tparam → x-tparam` rename map through the recursion and apply it to
//!   the `y` operand at each level. `rparams` (receiver type params) are not
//!   part of function-type identity, matching Go.
//! - `Named` identical: type-argument comparison (instantiation) is not
//!   yet ported, so we compare via `Origin` equality only — correct for
//!   the chunk-3 "all Nameds are uninstantiated" world.
//! - `is_generic`: returns `false` until Alias.tparams and Named.inst are
//!   ported.
//! - Union/Interface term-set identity uses structural
//!   [`identical`](identical) via upgraded `termlist` (D01).

use crate::hash::HashMap;
use crate::hash::HashSet;

use crate::alias::unalias_readonly;
use crate::arena::{ObjectArena, ObjectData, ObjectId, PackageArena, TypeArena, TypeData, TypeId};
use crate::basic::{
    BasicKind, IS_BOOLEAN, IS_COMPLEX, IS_CONST_TYPE, IS_FLOAT, IS_INTEGER, IS_NUMERIC, IS_ORDERED,
    IS_STRING, IS_UNSIGNED, IS_UNTYPED,
};
use crate::interface::interface_compute_typeset;
use crate::termlist;

// ----------------------------------------------------------------------------
// Validity

/// Reports whether `t` is a valid type (not `Typ[Invalid]`).
pub fn is_valid(arena: &TypeArena, t: TypeId) -> bool {
    let u = unalias_readonly(arena, t);
    match arena.get(u) {
        TypeData::Basic(b) => b.kind() != BasicKind::Invalid,
        _ => true,
    }
}

// ----------------------------------------------------------------------------
// Basic-info predicates
//
// These look at `t.Underlying()`; they don't look inside type parameters
// (matching Go's `isX` family). For the type-set-aware variants see the
// `allX` family below.

/// Reports whether `t.Underlying()` is a basic type whose info overlaps
/// `info`. Doesn't peek inside type parameters.
pub fn is_basic(arena: &TypeArena, t: TypeId, info: crate::basic::BasicInfo) -> bool {
    let u = t.underlying(arena);
    match arena.get(u) {
        TypeData::Basic(b) => (b.info().0 & info.0) != 0,
        _ => false,
    }
}

pub fn is_boolean(arena: &TypeArena, t: TypeId) -> bool {
    is_basic(arena, t, IS_BOOLEAN)
}
pub fn is_integer(arena: &TypeArena, t: TypeId) -> bool {
    is_basic(arena, t, IS_INTEGER)
}
pub fn is_unsigned(arena: &TypeArena, t: TypeId) -> bool {
    is_basic(arena, t, IS_UNSIGNED)
}
pub fn is_float(arena: &TypeArena, t: TypeId) -> bool {
    is_basic(arena, t, IS_FLOAT)
}
pub fn is_complex(arena: &TypeArena, t: TypeId) -> bool {
    is_basic(arena, t, IS_COMPLEX)
}
pub fn is_numeric(arena: &TypeArena, t: TypeId) -> bool {
    is_basic(arena, t, IS_NUMERIC)
}
pub fn is_string(arena: &TypeArena, t: TypeId) -> bool {
    is_basic(arena, t, IS_STRING)
}
pub fn is_integer_or_float(arena: &TypeArena, t: TypeId) -> bool {
    is_basic(arena, t, IS_INTEGER | IS_FLOAT)
}
pub fn is_const_type(arena: &TypeArena, t: TypeId) -> bool {
    is_basic(arena, t, IS_CONST_TYPE)
}

// ----------------------------------------------------------------------------
// Type-set-aware predicates (`allX`)
//
// The `isX` family above stops at `t.Underlying()`, so a type parameter never
// satisfies any of them. Go's `allX` family looks *inside* the parameter: the
// predicate must hold for every specific term of the constraint's type set.
// Without these, `func Sum[T ~int | ~float64](…) { total += x }` is rejected
// with "operator ADD not defined on operand" and the whole package goes
// ill-typed — i.e. every type-dependent analyzer silently reports nothing.

/// Reports whether `t.Underlying()` is a basic type whose info overlaps
/// `info`; if `t` is a type parameter, whether that holds for *every*
/// specific term of its type set.
///
/// A type set with no specific terms (e.g. `any`) yields `false`, matching
/// Go's `is(f)` calling `f(nil)` and `allBasic`'s `t != nil &&` guard.
///
/// Equivalent to `predicates.go::allBasic`.
pub fn all_basic(
    arena: &mut TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    t: TypeId,
    info: crate::basic::BasicInfo,
) -> bool {
    let u = unalias_readonly(arena, t);
    if !matches!(arena.get(u), TypeData::TypeParam(_)) {
        return is_basic(arena, t, info);
    }
    let iface = crate::typeparam::type_param_iface(arena, objects, packages, u);
    interface_compute_typeset(arena, objects, packages, iface);
    // Read the cache in place rather than via `interface_typeset`, which hands
    // back a clone: this runs on every operand of every operator.
    let types: &TypeArena = arena;
    let Some(tset) = (match types.get(iface) {
        TypeData::Interface(i) => i.cached_typeset(),
        _ => None,
    }) else {
        return false;
    };
    tset.is(|_tilde, term| match term {
        Some(ty) => is_basic(types, ty, info),
        None => false,
    })
}

pub fn all_boolean(a: &mut TypeArena, o: &ObjectArena, p: &PackageArena, t: TypeId) -> bool {
    all_basic(a, o, p, t, IS_BOOLEAN)
}
pub fn all_integer(a: &mut TypeArena, o: &ObjectArena, p: &PackageArena, t: TypeId) -> bool {
    all_basic(a, o, p, t, IS_INTEGER)
}
pub fn all_unsigned(a: &mut TypeArena, o: &ObjectArena, p: &PackageArena, t: TypeId) -> bool {
    all_basic(a, o, p, t, IS_UNSIGNED)
}
pub fn all_numeric(a: &mut TypeArena, o: &ObjectArena, p: &PackageArena, t: TypeId) -> bool {
    all_basic(a, o, p, t, IS_NUMERIC)
}
pub fn all_string(a: &mut TypeArena, o: &ObjectArena, p: &PackageArena, t: TypeId) -> bool {
    all_basic(a, o, p, t, IS_STRING)
}
pub fn all_ordered(a: &mut TypeArena, o: &ObjectArena, p: &PackageArena, t: TypeId) -> bool {
    all_basic(a, o, p, t, IS_ORDERED)
}
pub fn all_numeric_or_string(
    a: &mut TypeArena,
    o: &ObjectArena,
    p: &PackageArena,
    t: TypeId,
) -> bool {
    all_basic(a, o, p, t, IS_NUMERIC | IS_STRING)
}

// ----------------------------------------------------------------------------
// Identity-class predicates

/// Reports whether `t` has a name (basic, named, or type-parameter).
///
/// Equivalent to `hasName`. Safe to call on partially-set-up types.
pub fn has_name(arena: &TypeArena, t: TypeId) -> bool {
    let u = unalias_readonly(arena, t);
    matches!(
        arena.get(u),
        TypeData::Basic(_) | TypeData::Named(_) | TypeData::TypeParam(_)
    )
}

/// Reports whether `t` is a type literal — i.e. not `Named` and not a
/// `TypeParam`. Predeclared `Basic` counts as a literal.
///
/// Equivalent to `isTypeLit`.
pub fn is_type_lit(arena: &TypeArena, t: TypeId) -> bool {
    let u = unalias_readonly(arena, t);
    !matches!(arena.get(u), TypeData::Named(_) | TypeData::TypeParam(_))
}

/// Reports whether `t` is a generic (parameterized but not-yet-instantiated)
/// type — i.e. a generic `Named` or a generic `Alias` with no type arguments.
///
/// Equivalent to `predicates.go::isGeneric`.
pub fn is_generic(arena: &TypeArena, t: TypeId) -> bool {
    match arena.get(t) {
        // A parameterized alias is generic only if it has no instantiation yet.
        TypeData::Alias(a) => a.type_params().is_some() && a.type_args().is_none(),
        TypeData::Named(n) => !n.is_instance() && n.type_params().map_or(0, |tp| tp.len()) > 0,
        _ => false,
    }
}

/// Reports whether `t` is typed; i.e. not an untyped constant or boolean.
///
/// Equivalent to `isTyped`. Note this looks at `t` directly (not its
/// underlying) — alias/named types can't denote untyped types in Go.
pub fn is_typed(arena: &TypeArena, t: TypeId) -> bool {
    match arena.get(t) {
        TypeData::Basic(b) => (b.info().0 & IS_UNTYPED.0) == 0,
        _ => true,
    }
}

pub fn is_untyped(arena: &TypeArena, t: TypeId) -> bool {
    !is_typed(arena, t)
}

/// Reports whether `t` is an untyped numeric type.
pub fn is_untyped_numeric(arena: &TypeArena, t: TypeId) -> bool {
    match arena.get(t) {
        TypeData::Basic(b) => (b.info().0 & IS_UNTYPED.0) != 0 && (b.info().0 & IS_NUMERIC.0) != 0,
        _ => false,
    }
}

/// Reports whether `t`'s underlying type is an interface.
pub fn is_interface(arena: &TypeArena, t: TypeId) -> bool {
    matches!(arena.get(t.underlying(arena)), TypeData::Interface(_))
}

/// Reports whether `t` is a type parameter.
pub fn is_type_param(arena: &TypeArena, t: TypeId) -> bool {
    matches!(
        arena.get(unalias_readonly(arena, t)),
        TypeData::TypeParam(_)
    )
}

/// Reports whether `t` is an interface that isn't a type parameter.
pub fn is_non_type_param_interface(arena: &TypeArena, t: TypeId) -> bool {
    !is_type_param(arena, t) && is_interface(arena, t)
}

/// Reports whether two packages are the same.
///
/// Chunk-5 stub: we don't have `Package` yet, so this only handles the
/// "both nil" case. The full implementation compares `Package.path`.
pub fn same_pkg(a: Option<()>, b: Option<()>) -> bool {
    // Chunk-N: replace `()` with `PackageId` and compare paths.
    a.is_none() == b.is_none()
}

/// Reports whether `t` includes the `nil` value.
///
/// Equivalent to `hasNil`. Doesn't handle the TypeParam-typeset case yet
/// (that needs the `underIs` helper — chunk-N).
pub fn has_nil(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    t: TypeId,
) -> bool {
    let u = t.underlying(arena);
    match arena.get(u) {
        TypeData::Basic(b) => b.kind() == BasicKind::UnsafePointer,
        TypeData::Slice(_)
        | TypeData::Pointer(_)
        | TypeData::Signature(_)
        | TypeData::Map(_)
        | TypeData::Chan(_) => true,
        // A type parameter's underlying type is its constraint interface, so
        // it lands here — and it has nil when **every** term of its type set
        // does. Answering `false` for all of them (the shape this arm had)
        // rejected `*m == nil` on a `T ~map[K]V`, which is how tailscale's
        // `util/mak` and seven more of its packages went ill-typed.
        TypeData::Interface(_) => {
            if !is_type_param(arena, t) {
                return true;
            }
            // `underIs(t, func(u) { return u != nil && hasNil(u) })`. The terms
            // are snapshotted first: computing the type set needs the arena
            // mutably, and so does the recursive `has_nil` on each term.
            let mut unders: Vec<Option<TypeId>> = Vec::new();
            crate::under::typeset_iter(arena, oarena, parena, t, |_, u| {
                unders.push(u);
                true
            });
            // A constraint with no specific terms yields one `None`, which is
            // upstream's `u != nil` failing: `any` and `comparable` have no nil.
            !unders.is_empty()
                && unders
                    .iter()
                    .all(|u| u.is_some_and(|u| has_nil(arena, oarena, parena, u)))
        }
        _ => false,
    }
}

// ----------------------------------------------------------------------------
// is_valid_name

pub fn is_valid_name(s: &str) -> bool {
    for (i, ch) in s.chars().enumerate() {
        let ok = ch.is_alphabetic() || ch == '_' || (i > 0 && ch.is_numeric());
        if !ok {
            return false;
        }
    }
    !s.is_empty() && {
        let first = s.chars().next().unwrap();
        first.is_alphabetic() || first == '_'
    }
}

// ----------------------------------------------------------------------------
// Default / max_type

/// Returns the default "typed" type for an "untyped" type; returns `t`
/// unchanged for typed types.
///
/// Equivalent to `Default`. Doesn't handle UntypedRune (returns `int32`
/// from the predeclared table) — Go uses a distinct `universeRune`
/// `TypeName` which we'll have once the full universe lands.
pub fn default_type(arena: &TypeArena, table: &[TypeId], t: TypeId) -> TypeId {
    match arena.get(t) {
        TypeData::Basic(b) => match b.kind() {
            BasicKind::UntypedBool => table[BasicKind::Bool as usize],
            BasicKind::UntypedInt => table[BasicKind::Int as usize],
            BasicKind::UntypedRune => table[BasicKind::Int32 as usize], // see doc comment
            BasicKind::UntypedFloat => table[BasicKind::Float64 as usize],
            BasicKind::UntypedComplex => table[BasicKind::Complex128 as usize],
            BasicKind::UntypedString => table[BasicKind::String as usize],
            _ => t,
        },
        _ => t,
    }
}

/// The "largest" type that encompasses both `x` and `y`. Returns `None`
/// for incompatible non-equal types.
///
/// Equivalent to `maxType`. For untyped numerics, picks the type whose
/// `BasicKind` discriminant is larger (matching Go's enum order:
/// `UntypedInt < UntypedRune < UntypedFloat < UntypedComplex`).
pub fn max_type(arena: &TypeArena, x: TypeId, y: TypeId) -> Option<TypeId> {
    if x == y {
        return Some(x);
    }
    if is_untyped_numeric(arena, x) && is_untyped_numeric(arena, y) {
        let (xk, yk) = match (arena.get(x), arena.get(y)) {
            (TypeData::Basic(a), TypeData::Basic(b)) => (a.kind() as u8, b.kind() as u8),
            _ => return None,
        };
        return Some(if xk > yk { x } else { y });
    }
    None
}

// ----------------------------------------------------------------------------
// has_empty_typeset

/// Reports whether `t` is a type parameter with an empty type set.
/// Doesn't force computation, so may return a false negative if the type
/// set hasn't been computed yet (matches Go's docstring).
pub fn has_empty_typeset(arena: &TypeArena, t: TypeId) -> bool {
    let u = unalias_readonly(arena, t);
    let bound = match arena.get(u) {
        TypeData::TypeParam(tp) => tp.constraint(),
        _ => return false,
    };
    let Some(b) = bound else { return false };
    let bu = b.underlying(arena);
    match arena.get(bu) {
        TypeData::Interface(i) => i.cached_typeset().map_or(false, |ts| ts.is_empty()),
        _ => false,
    }
}

// ----------------------------------------------------------------------------
// Identical — structural type equality

/// Configuration knobs for [`identical_with`]. Defaults match Go's
/// zero-value `comparer{}`.
#[derive(Default, Clone, Copy)]
pub struct IdenticalCfg {
    pub ignore_tags: bool,
    pub ignore_invalids: bool,
}

/// Reports whether `x` and `y` are structurally identical types.
///
/// Equivalent to `Identical`. Takes `&mut TypeArena` because Interface
/// identity requires the type set (lazily computed and cached on first
/// access).
pub fn identical(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: TypeId,
    y: TypeId,
) -> bool {
    identical_with(arena, oarena, parena, x, y, IdenticalCfg::default())
}

/// Identity check with tunable knobs — `ignore_tags` skips struct tag
/// comparison; `ignore_invalids` treats `Typ[Invalid]` as identical to
/// anything (Go's "avoid follow-on errors" mode).
pub fn identical_with(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: TypeId,
    y: TypeId,
    cfg: IdenticalCfg,
) -> bool {
    let mut stack: Vec<(TypeId, TypeId)> = Vec::new();
    // Top-level identity has no in-flight type-parameter renaming; generic
    // signatures build their own map (see `identical_signatures`).
    let rename: HashMap<TypeId, TypeId> = HashMap::default();
    identical_inner(arena, oarena, parena, x, y, &mut stack, cfg, &rename)
}

fn identical_inner(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: TypeId,
    y: TypeId,
    stack: &mut Vec<(TypeId, TypeId)>,
    cfg: IdenticalCfg,
    rename: &HashMap<TypeId, TypeId>,
) -> bool {
    // Generic-signature comparison substitutes y's type parameters for x's
    // (see `identical_signatures`). Rather than materialising a fresh y via
    // `subst` (which would need `&mut ObjectArena`/`Context`), we thread the
    // `y-tparam → x-tparam` map down the recursion and apply it to the y
    // operand at every level — equivalent to substituting the whole subtree.
    let y = rename.get(&y).copied().unwrap_or(y);
    let x = unalias_readonly(arena, x);
    let y = unalias_readonly(arena, y);
    if x == y {
        return true;
    }
    if cfg.ignore_invalids && (!is_valid(arena, x) || !is_valid(arena, y)) {
        return true;
    }

    // Match on x's variant; for each, check y has the same variant and
    // structurally agree. We snapshot fields up-front so we don't hold a
    // borrow into the arena across recursive calls.
    use crate::TypeKind as K;
    let (xk, yk) = (x.kind(arena), y.kind(arena));
    if xk != yk {
        return false;
    }
    match xk {
        K::Basic => match (arena.get(x), arena.get(y)) {
            (TypeData::Basic(a), TypeData::Basic(b)) => a.kind() == b.kind(),
            _ => unreachable!(),
        },
        K::Array => {
            let (xl, xe) = match arena.get(x) {
                TypeData::Array(a) => (a.len(), a.elem()),
                _ => unreachable!(),
            };
            let (yl, ye) = match arena.get(y) {
                TypeData::Array(a) => (a.len(), a.elem()),
                _ => unreachable!(),
            };
            (xl < 0 || yl < 0 || xl == yl)
                && identical_inner(arena, oarena, parena, xe, ye, stack, cfg, rename)
        }
        K::Slice => {
            let xe = match arena.get(x) {
                TypeData::Slice(s) => s.elem(),
                _ => unreachable!(),
            };
            let ye = match arena.get(y) {
                TypeData::Slice(s) => s.elem(),
                _ => unreachable!(),
            };
            identical_inner(arena, oarena, parena, xe, ye, stack, cfg, rename)
        }
        K::Pointer => {
            let xe = match arena.get(x) {
                TypeData::Pointer(p) => p.elem(),
                _ => unreachable!(),
            };
            let ye = match arena.get(y) {
                TypeData::Pointer(p) => p.elem(),
                _ => unreachable!(),
            };
            identical_inner(arena, oarena, parena, xe, ye, stack, cfg, rename)
        }
        K::Map => {
            let (xk_, xe) = match arena.get(x) {
                TypeData::Map(m) => (m.key(), m.elem()),
                _ => unreachable!(),
            };
            let (yk_, ye) = match arena.get(y) {
                TypeData::Map(m) => (m.key(), m.elem()),
                _ => unreachable!(),
            };
            identical_inner(arena, oarena, parena, xk_, yk_, stack, cfg, rename)
                && identical_inner(arena, oarena, parena, xe, ye, stack, cfg, rename)
        }
        K::Chan => {
            let (xd, xe) = match arena.get(x) {
                TypeData::Chan(c) => (c.dir(), c.elem()),
                _ => unreachable!(),
            };
            let (yd, ye) = match arena.get(y) {
                TypeData::Chan(c) => (c.dir(), c.elem()),
                _ => unreachable!(),
            };
            xd == yd && identical_inner(arena, oarena, parena, xe, ye, stack, cfg, rename)
        }
        K::Struct => identical_structs(arena, oarena, parena, x, y, stack, cfg, rename),
        K::Tuple => identical_tuples(arena, oarena, parena, x, y, stack, cfg, rename),
        K::Signature => identical_signatures(arena, oarena, parena, x, y, stack, cfg, rename),
        K::Union => identical_unions(arena, oarena, parena, x, y),
        K::Interface => identical_interfaces(arena, oarena, parena, x, y, stack, cfg, rename),
        K::Named => identical_named(arena, oarena, parena, x, y, stack, cfg, rename),
        K::TypeParam => {
            // Caught by `x == y` above; differing TypeParams are never
            // identical (their TypeName objects differ).
            false
        }
        K::Alias => {
            // Unalias above already resolved aliases — we shouldn't reach
            // an Alias variant here. Defensive: equal only if same id
            // (handled above).
            false
        }
    }
}

fn identical_structs(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: TypeId,
    y: TypeId,
    stack: &mut Vec<(TypeId, TypeId)>,
    cfg: IdenticalCfg,
    rename: &HashMap<TypeId, TypeId>,
) -> bool {
    let (x_fields, x_tags) = match arena.get(x) {
        TypeData::Struct(s) => (
            (0..s.num_fields()).map(|i| s.field(i)).collect::<Vec<_>>(),
            (0..s.num_fields())
                .map(|i| s.tag(i).to_string())
                .collect::<Vec<_>>(),
        ),
        _ => unreachable!(),
    };
    let (y_fields, y_tags) = match arena.get(y) {
        TypeData::Struct(s) => (
            (0..s.num_fields()).map(|i| s.field(i)).collect::<Vec<_>>(),
            (0..s.num_fields())
                .map(|i| s.tag(i).to_string())
                .collect::<Vec<_>>(),
        ),
        _ => unreachable!(),
    };
    if x_fields.len() != y_fields.len() {
        return false;
    }
    for i in 0..x_fields.len() {
        let f = x_fields[i];
        let g = y_fields[i];

        // Embedded flag must match.
        if var_embedded(oarena, f) != var_embedded(oarena, g) {
            return false;
        }
        // Tags (unless ignored).
        if !cfg.ignore_tags && x_tags[i] != y_tags[i] {
            return false;
        }
        // Field name + package-qualification via sameId — unexported
        // field names from different packages are different identifiers.
        let g_name = g.name(oarena).to_string();
        let g_pkg = g.pkg(oarena);
        if !f.same_id(oarena, parena, g_pkg, &g_name, false) {
            return false;
        }
        let ftyp = f.typ(oarena).expect("Var must have typ");
        let gtyp = g.typ(oarena).expect("Var must have typ");
        if !identical_inner(arena, oarena, parena, ftyp, gtyp, stack, cfg, rename) {
            return false;
        }
    }
    true
}

/// Helper: read `Var.embedded` via the arena (returns false for
/// non-Var objects since `embedded` doesn't apply).
fn var_embedded(oarena: &ObjectArena, id: ObjectId) -> bool {
    match oarena.get(id) {
        ObjectData::Var(v) => v.embedded(),
        _ => false,
    }
}

fn identical_tuples(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: TypeId,
    y: TypeId,
    stack: &mut Vec<(TypeId, TypeId)>,
    cfg: IdenticalCfg,
    rename: &HashMap<TypeId, TypeId>,
) -> bool {
    let xv: Vec<ObjectId> = match arena.get(x) {
        TypeData::Tuple(t) => (0..t.len()).map(|i| t.at(i)).collect(),
        _ => unreachable!(),
    };
    let yv: Vec<ObjectId> = match arena.get(y) {
        TypeData::Tuple(t) => (0..t.len()).map(|i| t.at(i)).collect(),
        _ => unreachable!(),
    };
    if xv.len() != yv.len() {
        return false;
    }
    for i in 0..xv.len() {
        let v_typ = xv[i].typ(oarena).expect("tuple Var must have typ");
        let w_typ = yv[i].typ(oarena).expect("tuple Var must have typ");
        if !identical_inner(arena, oarena, parena, v_typ, w_typ, stack, cfg, rename) {
            return false;
        }
    }
    true
}

fn identical_signatures(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: TypeId,
    y: TypeId,
    stack: &mut Vec<(TypeId, TypeId)>,
    cfg: IdenticalCfg,
    rename: &HashMap<TypeId, TypeId>,
) -> bool {
    let (xv, xp, xr) = match arena.get(x) {
        TypeData::Signature(s) => (s.variadic(), s.params(), s.results()),
        _ => unreachable!(),
    };
    let (yv, yp, yr) = match arena.get(y) {
        TypeData::Signature(s) => (s.variadic(), s.params(), s.results()),
        _ => unreachable!(),
    };
    if xv != yv {
        return false;
    }

    // Two function types are identical if they have the same number of
    // parameters and result values, corresponding parameter and result types
    // are identical, and either both are variadic or neither is. Parameter and
    // result names are not required to match, and type parameters are
    // considered identical modulo renaming (Go: predicates.go `*Signature`).
    let xtparams: Vec<TypeId> = match arena.get(x) {
        TypeData::Signature(s) => s
            .type_params()
            .map(|l| l.list().to_vec())
            .unwrap_or_default(),
        _ => unreachable!(),
    };
    let ytparams: Vec<TypeId> = match arena.get(y) {
        TypeData::Signature(s) => s
            .type_params()
            .map(|l| l.list().to_vec())
            .unwrap_or_default(),
        _ => unreachable!(),
    };
    if xtparams.len() != ytparams.len() {
        return false;
    }

    // Effective renaming: inherit any in-flight map (nested generic
    // signatures) and add this signature's own y-tparam → x-tparam
    // mappings — the analogue of Go's `makeSubstMap(ytparams, xtparams)`.
    // For non-generic signatures this is just a copy of `rename`.
    let mut merged = rename.clone();
    for (yt, xt) in ytparams.iter().zip(xtparams.iter()) {
        merged.insert(*yt, *xt);
    }
    let eff = &merged;

    // Constraints must be pair-wise identical, after substitution.
    for (xt, yt) in xtparams.iter().zip(ytparams.iter()) {
        let xb = crate::typeparam::type_param_constraint(arena, *xt);
        let yb = crate::typeparam::type_param_constraint(arena, *yt);
        let eq = match (xb, yb) {
            (None, None) => true,
            (Some(a), Some(b)) => identical_inner(arena, oarena, parena, a, b, stack, cfg, eff),
            _ => false,
        };
        if !eq {
            return false;
        }
    }

    let params_eq = match (xp, yp) {
        (None, None) => true,
        (Some(a), Some(b)) => identical_inner(arena, oarena, parena, a, b, stack, cfg, eff),
        _ => false,
    };
    if !params_eq {
        return false;
    }
    match (xr, yr) {
        (None, None) => true,
        (Some(a), Some(b)) => identical_inner(arena, oarena, parena, a, b, stack, cfg, eff),
        _ => false,
    }
}

fn identical_unions(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: TypeId,
    y: TypeId,
) -> bool {
    // Go: computeUnionTypeSet on both, then .terms.equal (D01).
    let mut union_sets = HashMap::default();
    let xset = crate::typeset::compute_union_type_set(arena, oarena, parena, &mut union_sets, x);
    let yset = crate::typeset::compute_union_type_set(arena, oarena, parena, &mut union_sets, y);
    termlist::equal(arena, oarena, parena, &xset.terms, &yset.terms)
}

fn identical_interfaces(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: TypeId,
    y: TypeId,
    stack: &mut Vec<(TypeId, TypeId)>,
    cfg: IdenticalCfg,
    rename: &HashMap<TypeId, TypeId>,
) -> bool {
    // Compute type sets for both (lazy compute via &mut).
    interface_compute_typeset(arena, oarena, parena, x);
    interface_compute_typeset(arena, oarena, parena, y);

    // Snapshot type set fields.
    let (xcomp, xterms, xmethods) = match arena.get(x) {
        TypeData::Interface(i) => {
            let ts = i.tset.as_ref().expect("computed above");
            (ts.comparable(), ts.terms.clone(), ts.methods().to_vec())
        }
        _ => unreachable!(),
    };
    let (ycomp, yterms, ymethods) = match arena.get(y) {
        TypeData::Interface(i) => {
            let ts = i.tset.as_ref().expect("computed above");
            (ts.comparable(), ts.terms.clone(), ts.methods().to_vec())
        }
        _ => unreachable!(),
    };
    if xcomp != ycomp {
        return false;
    }
    if !termlist::equal(arena, oarena, parena, &xterms, &yterms) {
        return false;
    }
    if xmethods.len() != ymethods.len() {
        return false;
    }

    // ifacePair cycle detection: if we've already seen this (x, y) pair
    // higher up the stack, they're considered identical (otherwise the
    // recursion would have stopped there).
    let pair = (x, y);
    let pair_swapped = (y, x);
    if stack.iter().any(|&p| p == pair || p == pair_swapped) {
        return true;
    }
    stack.push(pair);
    let ok = (|| -> bool {
        // Methods are pre-sorted (canonical Object.cmp order). Compare
        // position-by-position using Id() so that two unexported methods
        // with the same spelling but different packages are not equal.
        for i in 0..xmethods.len() {
            let xm = xmethods[i];
            let ym = ymethods[i];
            if xm.id(oarena, parena) != ym.id(oarena, parena) {
                return false;
            }
            let xt = xm.typ(oarena);
            let yt = ym.typ(oarena);
            match (xt, yt) {
                (Some(a), Some(b)) => {
                    if !identical_inner(arena, oarena, parena, a, b, stack, cfg, rename) {
                        return false;
                    }
                }
                (None, None) => {}
                _ => return false,
            }
        }
        true
    })();
    stack.pop();
    ok
}

fn identical_named(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: TypeId,
    y: TypeId,
    stack: &mut Vec<(TypeId, TypeId)>,
    cfg: IdenticalCfg,
    rename: &HashMap<TypeId, TypeId>,
) -> bool {
    // Two named types are identical if (a) their Origin types are equal
    // (same declaration), AND (b) their TypeArgs (if any) are pairwise
    // identical. For non-instantiated types TypeArgs is None — the
    // origin equality suffices.
    let x_origin = crate::named::named_origin(arena, x);
    let y_origin = crate::named::named_origin(arena, y);
    let x_obj = match arena.get(x_origin) {
        TypeData::Named(n) => n.obj(),
        _ => unreachable!(),
    };
    let y_obj = match arena.get(y_origin) {
        TypeData::Named(n) => n.obj(),
        _ => unreachable!(),
    };
    if x_obj != y_obj {
        return false;
    }
    // Snapshot TypeArgs (clone the Vec<TypeId>) to avoid holding the
    // arena borrow across recursive identity checks.
    let xa: Vec<TypeId> = crate::named::named_type_args(arena, x)
        .map(|l| l.list().to_vec())
        .unwrap_or_default();
    let ya: Vec<TypeId> = crate::named::named_type_args(arena, y)
        .map(|l| l.list().to_vec())
        .unwrap_or_default();
    if xa.len() != ya.len() {
        return false;
    }
    for i in 0..xa.len() {
        if !identical_inner(arena, oarena, parena, xa[i], ya[i], stack, cfg, rename) {
            return false;
        }
    }
    true
}

/// Reports whether two named types originated in the same declaration.
///
/// Equivalent to `identicalOrigin`. With instantiation support (chunk 9),
/// this walks both types' `Origin()` first and then compares the
/// resulting TypeName objects.
pub fn identical_origin(arena: &TypeArena, x: TypeId, y: TypeId) -> bool {
    let xo = crate::named::named_origin(arena, x);
    let yo = crate::named::named_origin(arena, y);
    let xobj = match arena.get(xo) {
        TypeData::Named(n) => n.obj(),
        _ => return false,
    };
    let yobj = match arena.get(yo) {
        TypeData::Named(n) => n.obj(),
        _ => return false,
    };
    xobj == yobj
}

// ----------------------------------------------------------------------------
// Comparable / comparable_type — all now take &PackageArena since the
// Interface arm calls compute_interface_type_set which requires it.

/// Reports whether values of type `t` are comparable.
///
/// Equivalent to `Comparable`.
pub fn comparable(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    t: TypeId,
) -> bool {
    let mut seen = HashSet::default();
    comparable_type(arena, oarena, parena, t, true, &mut seen).is_ok()
}

/// Reports comparability with an error message on failure. `dynamic`
/// matches Go: when set, non-type-parameter interfaces are always
/// considered comparable.
///
/// Returns `Ok(())` if `t` is comparable, or `Err(message)` describing
/// why not.
/// Does every type in interface `u`'s type set compare?
///
/// Port of `_TypeSet.IsComparable`. The `comparable` **flag** only records that
/// the interface embedded `comparable` literally; when the set has terms, the
/// answer has to be *computed* from them. Reading the flag alone made
/// `cmp.Ordered` — whose terms are all strictly comparable basic types — fail
/// to satisfy `comparable`, so every `Set[T]` in kubernetes'
/// `apimachinery/pkg/util/sets` and `pkg/api/validate` was rejected and the
/// packages went ill-typed.
pub fn typeset_is_comparable(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    u: TypeId,
    seen: &mut HashSet<TypeId>,
) -> bool {
    crate::interface::interface_compute_typeset(arena, oarena, parena, u);
    let (terms_all, flag, terms) = match arena.get(u) {
        TypeData::Interface(i) => {
            let ts = i.tset.as_ref().expect("typeset computed above");
            (
                crate::termlist::is_all(&ts.terms),
                ts.comparable(),
                ts.terms.clone(),
            )
        }
        _ => return false,
    };
    if terms_all {
        return flag;
    }
    // `s.is(...)`: true for every specific term, and `is` calls the predicate
    // once with `nil` when there are none — which the `t != nil` guard then
    // fails, so a term-less (empty) set is not comparable.
    let mut any = false;
    for slot in terms.iter() {
        let Some(t) = slot.as_ref() else { continue };
        any = true;
        // A term with no type is the `𝓤` (all types) term; `terms.isAll()`
        // above already handled the only set that is only that, so reaching it
        // here means the set is not comparable.
        let Some(typ) = t.typ else { return false };
        if comparable_type(arena, oarena, parena, typ, false, seen).is_err() {
            return false;
        }
    }
    any
}

pub fn comparable_type(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    t: TypeId,
    dynamic: bool,
    seen: &mut HashSet<TypeId>,
) -> Result<(), String> {
    if seen.contains(&t) {
        return Ok(());
    }
    seen.insert(t);

    let u = t.underlying(arena);
    match arena.get(u) {
        TypeData::Basic(b) => {
            if b.kind() == BasicKind::UntypedNil {
                return Err(String::new());
            }
            // Assume invalid types are comparable to avoid follow-on errors.
            Ok(())
        }
        TypeData::Pointer(_) | TypeData::Chan(_) => Ok(()),
        TypeData::Struct(_) => {
            // Snapshot fields then recurse.
            let fields: Vec<ObjectId> = match arena.get(u) {
                TypeData::Struct(s) => (0..s.num_fields()).map(|i| s.field(i)).collect(),
                _ => unreachable!(),
            };
            for f in fields {
                let ftyp = f.typ(oarena).expect("Var must have typ");
                if comparable_type(arena, oarena, parena, ftyp, dynamic, seen).is_err() {
                    let name = match oarena.get(f) {
                        ObjectData::Var(v) => v.name().to_string(),
                        _ => String::new(),
                    };
                    return Err(format!("struct containing {} cannot be compared", name));
                }
            }
            Ok(())
        }
        TypeData::Array(_) => {
            let elem = match arena.get(u) {
                TypeData::Array(a) => a.elem(),
                _ => unreachable!(),
            };
            comparable_type(arena, oarena, parena, elem, dynamic, seen)
                .map_err(|_| format!("array element is not comparable"))
        }
        TypeData::Interface(_) => {
            // dynamic + non-TypeParam interface: always comparable.
            if dynamic && !is_type_param(arena, t) {
                return Ok(());
            }
            // Otherwise, ask the type set — `IsComparable`, not the raw flag.
            if typeset_is_comparable(arena, oarena, parena, u, seen) {
                return Ok(());
            }
            interface_compute_typeset(arena, oarena, parena, u);
            let (is_empty, is_comp) = match arena.get(u) {
                TypeData::Interface(i) => {
                    let ts = i.tset.as_ref().unwrap();
                    (ts.is_empty(), ts.comparable())
                }
                _ => unreachable!(),
            };
            if is_comp {
                Ok(())
            } else if is_empty {
                Err("empty type set".to_string())
            } else {
                Err("incomparable types in type set".to_string())
            }
        }
        _ => Err(String::new()),
    }
}
