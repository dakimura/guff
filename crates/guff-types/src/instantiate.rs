//! Port of `cmd/compile/internal/types2/instantiate.go`.
//!
//! Public [`instantiate`] API for creating instances of generic `Named`,
//! `Alias`, and `Signature` types from type arguments. Internally
//! [`new_named_instance`] / [`new_alias_instance`] /
//! [`new_signature_instance`] do the actual work and are also called
//! from [`crate::subst`] when substitution needs to instantiate.
//!
//! Chunk-9 simplifications:
//! - **No validation.** Go's `Instantiate` accepts a `validate` flag that
//!   triggers constraint-satisfaction checks via `unify.go`. We don't
//!   have unify yet; `instantiate` always behaves like `validate=false`.
//! - **Eager Named expansion.** Go expands a Named instance's underlying
//!   lazily (via `Named.Underlying()`). We expand eagerly here, registering
//!   the placeholder in the Context first to break cycles.
//! - **Lazy method expansion.** A Named instance stores no expanded method
//!   list of its own. Instead, method resolution on instances is done on
//!   demand at selection time (chunk 67 / D05): `named_lookup_method` searches
//!   the origin's methods, and `Checker::method_sig_for_recv` substitutes the
//!   instance's type arguments into the selected method's signature. This
//!   mirrors Go, whose `Named.Method(i)` expands a copy of the origin's i-th
//!   method rather than mutating any shared list.

use crate::arena::{ObjectArena, TypeArena, TypeData, TypeId};
use crate::context::Context;
use crate::named::Instance;
use crate::subst::{make_subst_map, subst};
use crate::typelists::new_type_list;

/// Instantiate `orig` (a generic `Named`, `Alias`, or `Signature`) with
/// the supplied type arguments.
///
/// Equivalent to `types2.Instantiate` minus the `validate` path.
///
/// Under incomplete hybrid type info, length mismatches / empty `targs` /
/// non-generic `orig` soft-return `orig` unchanged (same doctrine as
/// [`crate::subst::subst_named`]) rather than panicking — SSA must not abort
/// the whole package build.
pub fn instantiate(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    ctxt: &mut Context,
    orig: TypeId,
    targs: Vec<TypeId>,
) -> TypeId {
    if targs.is_empty() {
        return orig;
    }
    instance(arena, oarena, ctxt, orig, targs)
}

/// Internal dispatch. Equivalent to `Checker.instance`.
fn instance(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    ctxt: &mut Context,
    orig: TypeId,
    targs: Vec<TypeId>,
) -> TypeId {
    // Dedup via context.
    if let Some(existing) = ctxt.lookup(orig, &targs) {
        return existing;
    }
    let kind = orig.kind(arena);
    use crate::TypeKind as K;
    match kind {
        K::Named => new_named_instance(arena, oarena, ctxt, orig, targs),
        K::Alias => new_alias_instance(arena, oarena, ctxt, orig, targs),
        K::Signature => new_signature_instance(arena, oarena, ctxt, orig, targs),
        // Incomplete hybrid info / wrong kind — soft-return origin.
        _ => orig,
    }
}

/// Construct a new Named instance: `orig[targs...]`.
///
/// Equivalent to `(*Checker).newNamedInstance` + eager
/// `Named.expandRHS`. The result is registered with `ctxt` so concurrent
/// instantiations of the same `(orig, targs)` return the same TypeId.
///
/// Soft-returns `orig` when `orig` is not Named or when `targs`/`tparams`
/// lengths disagree (incomplete hybrid info — error reported elsewhere).
pub fn new_named_instance(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    ctxt: &mut Context,
    orig: TypeId,
    targs: Vec<TypeId>,
) -> TypeId {
    // Snapshot origin metadata.
    let (orig_obj, orig_from_rhs, orig_tparams) = match arena.get(orig) {
        TypeData::Named(n) => {
            let tparams_list: Vec<TypeId> = n
                .tparams
                .as_ref()
                .map(|l| l.list().to_vec())
                .unwrap_or_default();
            (n.obj(), n.from_rhs(), tparams_list)
        }
        _ => return orig,
    };
    if targs.len() != orig_tparams.len() {
        return orig; // mismatch — error reported elsewhere
    }

    // Allocate the new Named (placeholder underlying — will fill via
    // substitution below).
    let new_id = crate::named::new_named(arena, oarena, orig_obj, None, vec![]);

    // Snapshot origin's tparams clone before re-borrowing mutably.
    let orig_tparams_list = match arena.get(orig) {
        TypeData::Named(o) => o.tparams.clone(),
        _ => unreachable!(),
    };
    // Attach instance metadata immediately so cycles break here.
    let targs_list = new_type_list(targs.clone()).expect("non-empty");
    if let TypeData::Named(n) = arena.get_mut(new_id) {
        n.inst = Some(Instance {
            orig,
            targs: targs_list,
        });
        // Pull tparams across so accessors work; same list as the origin.
        n.tparams = orig_tparams_list;
    }

    // Register in context BEFORE expansion so any recursive references
    // (e.g. `type T[P] struct { next *T[P] }`) short-circuit here.
    ctxt.update(orig, targs.clone(), new_id);

    // Expand the underlying via substitution of orig's fromRHS.
    if let Some(rhs) = orig_from_rhs {
        let smap = make_subst_map(&orig_tparams, &targs);
        let new_rhs = subst(arena, oarena, &smap, Some(new_id), ctxt, rhs);
        // Underlying can't be a Named — but the substituted RHS could be
        // (e.g. `type T[P] U[P]`). For chunk 9 we only set the underlying
        // if the substituted RHS isn't itself a Named; otherwise leave
        // underlying=None and let TypeId::underlying re-dispatch.
        let new_rhs_is_named = matches!(arena.get(new_rhs), TypeData::Named(_));
        if !new_rhs_is_named {
            crate::named::set_underlying(arena, new_id, new_rhs);
        }
    }

    // Methods are NOT expanded here: an instance created during package-level
    // struct-field checking may precede its origin's method-signature
    // resolution, so an eager copy could capture an unresolved signature.
    // Method resolution is done lazily at consumption time — see
    // `Checker::method_sig_for_recv` (selection) and `missing_method`
    // (interface satisfaction), which substitute the instance's type arguments
    // into the origin method's signature after it is fully resolved.
    new_id
}

/// Construct a new Alias instance: `orig[targs...]`.
///
/// Equivalent to `(*Checker).newAliasInstance`.
pub fn new_alias_instance(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    ctxt: &mut Context,
    orig: TypeId,
    targs: Vec<TypeId>,
) -> TypeId {
    // Snapshot.
    let (orig_obj, orig_rhs, orig_tparams) = match arena.get(orig) {
        TypeData::Alias(a) => {
            let tparams_list: Vec<TypeId> = a
                .tparams
                .as_ref()
                .map(|l| l.list().to_vec())
                .unwrap_or_default();
            (a.obj(), a.rhs(), tparams_list)
        }
        _ => return orig,
    };
    if targs.len() != orig_tparams.len() {
        return orig; // mismatch — error reported elsewhere
    }

    // Substitute in the RHS.
    let new_rhs = match orig_rhs {
        Some(r) => {
            let smap = make_subst_map(&orig_tparams, &targs);
            Some(subst(arena, oarena, &smap, None, ctxt, r))
        }
        None => None,
    };

    // Create a fresh TypeName for the instance (sharing the origin's
    // name; the resulting instance still reports orig.obj as its
    // Origin().Obj() via the orig back-pointer).
    //
    // Go's `newAliasInstance` builds it as `NewTypeName(pos, orig.obj.pkg,
    // orig.obj.name, nil)`: the package and position come across too. Leaving
    // `pkg` unset makes the instance indistinguishable from a predeclared
    // type — `Object.Pkg() == nil` is exactly how callers spell "builtin", so
    // e.g. revive's `exportedType` waves through every instance of an
    // unexported generic alias.
    let name = orig_obj.name(oarena).to_string();
    let inst_typename = crate::object::type_name::new_type_name(oarena, name, None);
    if let Some(pkg) = orig_obj.pkg(oarena) {
        inst_typename.set_pkg(oarena, pkg);
    }
    inst_typename.set_pos(oarena, orig_obj.pos(oarena));
    let new_id = crate::alias::new_alias(arena, oarena, inst_typename, new_rhs);

    // Snapshot origin's tparams.
    let orig_tparams_list = match arena.get(orig) {
        TypeData::Alias(o) => o.tparams.clone(),
        _ => unreachable!(),
    };
    // Attach instance metadata.
    let targs_list = new_type_list(targs.clone()).expect("non-empty");
    if let TypeData::Alias(a) = arena.get_mut(new_id) {
        a.orig = Some(orig);
        a.targs = Some(targs_list);
        a.tparams = orig_tparams_list;
    }

    ctxt.update(orig, targs, new_id);
    new_id
}

/// Construct a new Signature instance: `orig[targs...]`.
///
/// Equivalent to `(*Checker).newSignatureInstance` (Go's actual path is
/// via `subst.go`'s direct return of a substituted `*Signature` with
/// `tparams=nil`).
pub fn new_signature_instance(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    ctxt: &mut Context,
    orig: TypeId,
    targs: Vec<TypeId>,
) -> TypeId {
    // Snapshot.
    let (recv, params, results, variadic, tparams) = match arena.get(orig) {
        TypeData::Signature(s) => {
            let tparams: Vec<TypeId> = s
                .tparams
                .as_ref()
                .map(|l| l.list().to_vec())
                .unwrap_or_default();
            (s.recv(), s.params(), s.results(), s.variadic(), tparams)
        }
        _ => return orig,
    };
    if targs.len() != tparams.len() {
        return orig; // mismatch — error reported elsewhere
    }

    let smap = make_subst_map(&tparams, &targs);
    let new_params = match params {
        Some(p) => Some(subst(arena, oarena, &smap, None, ctxt, p)),
        None => None,
    };
    let new_results = match results {
        Some(r) => Some(subst(arena, oarena, &smap, None, ctxt, r)),
        None => None,
    };
    // Instantiated signatures lose their type parameters (Go drops them).
    let new_id = crate::signature::new_signature_type(
        arena,
        recv,
        &[],
        &[],
        new_params,
        new_results,
        variadic,
    );
    ctxt.update(orig, targs, new_id);
    new_id
}
