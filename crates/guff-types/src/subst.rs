//! Port of `cmd/compile/internal/types2/subst.go`.
//!
//! Recursive substitution of type parameters with type arguments. The
//! workhorse [`subst`] walks every type variant and rebuilds only the
//! parts that change.
//!
//! Chunk-9 simplifications:
//! - **Receiver substitution in Signatures** — Go preserves `recv`
//!   verbatim because substitution happens inside Named/Interface
//!   expansion which handles the receiver separately. We do the same.
//! - **Interface receiver back-fill (`replaceRecvType`)** — Go rewrites
//!   embedded interface methods whose receiver was the original
//!   interface so they point at the new one. We defer that until
//!   needed; the chunk-2 deferral on interface method-receivers makes
//!   this safe for our test scenarios.
//! - **`expanding`** — Go threads the in-flight Named through subst so
//!   the substituter knows which instance is being expanded (used by
//!   `instance()`). We pass it through too.
//! - Container substitution rebuilds the children eagerly; we never
//!   return the original unchanged unless the substitution map is
//!   empty AND no children changed.

use std::collections::HashMap;

use crate::arena::{ObjectArena, ObjectData, ObjectId, TypeArena, TypeData, TypeId};
use crate::context::Context;

/// Substitution map: `TypeParam TypeId` → replacement `TypeId`.
///
/// Equivalent to `types2.substMap`. Use [`make_subst_map`] to build from
/// parallel parameter / argument lists.
pub type SubstMap = HashMap<TypeId, TypeId>;

/// Build a substitution map from `tpars[i] → targs[i]`.
///
/// Equivalent to `makeSubstMap`. Panics if the lists have different
/// lengths.
pub fn make_subst_map(tpars: &[TypeId], targs: &[TypeId]) -> SubstMap {
    assert_eq!(tpars.len(), targs.len(), "make_subst_map: length mismatch");
    let mut m = HashMap::with_capacity(tpars.len());
    for (t, a) in tpars.iter().zip(targs.iter()) {
        m.insert(*t, *a);
    }
    m
}

/// Look up a TypeParam's substitution. Returns the original `tp` if no
/// entry exists (matching Go's `substMap.lookup`).
pub fn subst_lookup(smap: &SubstMap, tp: TypeId) -> TypeId {
    smap.get(&tp).copied().unwrap_or(tp)
}

/// Substitute type parameters in `typ` according to `smap`.
///
/// `expanding` is `Some(Named)` when called from inside `instance` to
/// help break cycles via `ctxt`.
///
/// At least one of `expanding` or `ctxt` must be `Some` (Go's invariant).
///
/// Equivalent to `Checker.subst`.
pub fn subst(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    smap: &SubstMap,
    expanding: Option<TypeId>,
    ctxt: &mut Context,
    typ: TypeId,
) -> TypeId {
    if smap.is_empty() {
        return typ;
    }
    // Hot paths.
    match arena.get(typ) {
        TypeData::Basic(_) => return typ,
        TypeData::TypeParam(_) => return subst_lookup(smap, typ),
        _ => {}
    }
    subst_typ(arena, oarena, smap, expanding, ctxt, typ)
}

/// Recursive entry point. Returns either the same `typ` (no substitution
/// occurred) or a freshly-allocated TypeId with substituted children.
fn subst_typ(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    smap: &SubstMap,
    expanding: Option<TypeId>,
    ctxt: &mut Context,
    typ: TypeId,
) -> TypeId {
    // Snapshot the variant + relevant fields before recursing, to avoid
    // holding a borrow into the arena across recursive substitutions.
    let kind = typ.kind(arena);
    use crate::TypeKind as K;
    match kind {
        K::Basic => typ,

        K::TypeParam => subst_lookup(smap, typ),

        K::Array => {
            let (len, elem) = match arena.get(typ) {
                TypeData::Array(a) => (a.len(), a.elem()),
                _ => unreachable!(),
            };
            let new_elem = subst(arena, oarena, smap, expanding, ctxt, elem);
            if new_elem == elem {
                typ
            } else {
                crate::array::new_array(arena, new_elem, len)
            }
        }

        K::Slice => {
            let elem = match arena.get(typ) {
                TypeData::Slice(s) => s.elem(),
                _ => unreachable!(),
            };
            let new_elem = subst(arena, oarena, smap, expanding, ctxt, elem);
            if new_elem == elem {
                typ
            } else {
                crate::slice::new_slice(arena, new_elem)
            }
        }

        K::Pointer => {
            let base = match arena.get(typ) {
                TypeData::Pointer(p) => p.elem(),
                _ => unreachable!(),
            };
            let new_base = subst(arena, oarena, smap, expanding, ctxt, base);
            if new_base == base {
                typ
            } else {
                crate::pointer::new_pointer(arena, new_base)
            }
        }

        K::Map => {
            let (key, elem) = match arena.get(typ) {
                TypeData::Map(m) => (m.key(), m.elem()),
                _ => unreachable!(),
            };
            let new_key = subst(arena, oarena, smap, expanding, ctxt, key);
            let new_elem = subst(arena, oarena, smap, expanding, ctxt, elem);
            if new_key == key && new_elem == elem {
                typ
            } else {
                crate::map::new_map(arena, new_key, new_elem)
            }
        }

        K::Chan => {
            let (dir, elem) = match arena.get(typ) {
                TypeData::Chan(c) => (c.dir(), c.elem()),
                _ => unreachable!(),
            };
            let new_elem = subst(arena, oarena, smap, expanding, ctxt, elem);
            if new_elem == elem {
                typ
            } else {
                crate::chan::new_chan(arena, dir, new_elem)
            }
        }

        K::Tuple => subst_tuple(arena, oarena, smap, expanding, ctxt, typ),

        K::Struct => subst_struct(arena, oarena, smap, expanding, ctxt, typ),

        K::Signature => subst_signature(arena, oarena, smap, expanding, ctxt, typ),

        K::Union => subst_union(arena, oarena, smap, expanding, ctxt, typ),

        K::Interface => subst_interface(arena, oarena, smap, expanding, ctxt, typ),

        K::Alias => subst_alias(arena, oarena, smap, expanding, ctxt, typ),

        K::Named => subst_named(arena, oarena, smap, expanding, ctxt, typ),
    }
}

fn subst_tuple(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    smap: &SubstMap,
    expanding: Option<TypeId>,
    ctxt: &mut Context,
    typ: TypeId,
) -> TypeId {
    let vars: Vec<ObjectId> = match arena.get(typ) {
        TypeData::Tuple(t) => (0..t.len()).map(|i| t.at(i)).collect(),
        _ => unreachable!(),
    };
    let mut changed = false;
    let mut new_vars = Vec::with_capacity(vars.len());
    for v in &vars {
        let new_v = subst_var(arena, oarena, smap, expanding, ctxt, *v);
        if new_v != *v {
            changed = true;
        }
        new_vars.push(new_v);
    }
    if !changed {
        return typ;
    }
    crate::tuple::new_tuple(arena, &new_vars).unwrap_or(typ)
}

fn subst_struct(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    smap: &SubstMap,
    expanding: Option<TypeId>,
    ctxt: &mut Context,
    typ: TypeId,
) -> TypeId {
    let (fields, tags) = match arena.get(typ) {
        TypeData::Struct(s) => (
            (0..s.num_fields()).map(|i| s.field(i)).collect::<Vec<_>>(),
            (0..s.num_fields())
                .map(|i| s.tag(i).to_string())
                .collect::<Vec<_>>(),
        ),
        _ => unreachable!(),
    };
    let mut changed = false;
    let mut new_fields = Vec::with_capacity(fields.len());
    for f in &fields {
        let new_f = subst_var(arena, oarena, smap, expanding, ctxt, *f);
        if new_f != *f {
            changed = true;
        }
        new_fields.push(new_f);
    }
    if !changed {
        return typ;
    }
    crate::r#struct::new_struct(arena, new_fields, tags)
}

fn subst_signature(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    smap: &SubstMap,
    expanding: Option<TypeId>,
    ctxt: &mut Context,
    typ: TypeId,
) -> TypeId {
    let (recv, params, results, variadic) = match arena.get(typ) {
        TypeData::Signature(s) => (s.recv(), s.params(), s.results(), s.variadic()),
        _ => unreachable!(),
    };
    let new_params = match params {
        Some(p) => Some(subst(arena, oarena, smap, expanding, ctxt, p)),
        None => None,
    };
    let new_results = match results {
        Some(r) => Some(subst(arena, oarena, smap, expanding, ctxt, r)),
        None => None,
    };
    if new_params == params && new_results == results {
        return typ;
    }
    // Note: receiver is preserved verbatim (Go does the same — Named/Interface
    // expansion handles receiver back-fill separately).
    crate::signature::new_signature_type(arena, recv, &[], &[], new_params, new_results, variadic)
}

fn subst_union(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    smap: &SubstMap,
    expanding: Option<TypeId>,
    ctxt: &mut Context,
    typ: TypeId,
) -> TypeId {
    let terms: Vec<crate::union::Term> = match arena.get(typ) {
        TypeData::Union(u) => (0..u.len()).map(|i| u.term(i).clone()).collect(),
        _ => unreachable!(),
    };
    let mut changed = false;
    let mut new_terms = Vec::with_capacity(terms.len());
    for t in &terms {
        let inner = subst(arena, oarena, smap, expanding, ctxt, t.typ());
        if inner != t.typ() {
            changed = true;
        }
        new_terms.push(crate::union::new_term(t.tilde(), inner));
    }
    if !changed {
        return typ;
    }
    crate::union::new_union(arena, new_terms)
}

fn subst_interface(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    smap: &SubstMap,
    expanding: Option<TypeId>,
    ctxt: &mut Context,
    typ: TypeId,
) -> TypeId {
    // Snapshot.
    let (methods, embeddeds) = match arena.get(typ) {
        TypeData::Interface(i) => (i.methods.clone(), i.embeddeds.clone()),
        _ => unreachable!(),
    };
    let mut methods_changed = false;
    let mut new_methods = Vec::with_capacity(methods.len());
    for m in &methods {
        let new_m = subst_func(arena, oarena, smap, expanding, ctxt, *m);
        if new_m != *m {
            methods_changed = true;
        }
        new_methods.push(new_m);
    }
    let mut embeds_changed = false;
    let mut new_embeds = Vec::with_capacity(embeddeds.len());
    for e in &embeddeds {
        let new_e = subst(arena, oarena, smap, expanding, ctxt, *e);
        if new_e != *e {
            embeds_changed = true;
        }
        new_embeds.push(new_e);
    }
    if !methods_changed && !embeds_changed {
        return typ;
    }
    crate::interface::new_interface_type(arena, new_methods, new_embeds)
    // Chunk-9: skipping replaceRecvType (interface method receiver back-fill);
    // chunk-2 already deferred receiver wiring for interface methods.
}

fn subst_alias(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    smap: &SubstMap,
    expanding: Option<TypeId>,
    ctxt: &mut Context,
    typ: TypeId,
) -> TypeId {
    // Snapshot.
    let (orig_id, targs_opt) = match arena.get(typ) {
        TypeData::Alias(a) => (a.orig.unwrap_or(typ), a.targs.clone()),
        _ => unreachable!(),
    };
    let tparams_len = match arena.get(orig_id) {
        TypeData::Alias(a) => a.tparams.as_ref().map_or(0, |l| l.len()),
        _ => 0,
    };
    if tparams_len == 0 {
        return typ; // not parameterised
    }
    let targs_list = match &targs_opt {
        Some(l) => l.list().to_vec(),
        None => return typ,
    };
    if targs_list.len() != tparams_len {
        return typ; // mismatch — error reported elsewhere
    }
    // Substitute each existing type-arg; only re-instantiate if any changed.
    let mut changed = false;
    let mut new_targs = Vec::with_capacity(targs_list.len());
    for ta in &targs_list {
        let new_ta = subst(arena, oarena, smap, expanding, ctxt, *ta);
        if new_ta != *ta {
            changed = true;
        }
        new_targs.push(new_ta);
    }
    if !changed {
        return typ;
    }
    crate::instantiate::new_alias_instance(arena, oarena, ctxt, orig_id, new_targs)
}

fn subst_named(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    smap: &SubstMap,
    expanding: Option<TypeId>,
    ctxt: &mut Context,
    typ: TypeId,
) -> TypeId {
    // Snapshot. `Origin` gives us either self (for declared Named) or the
    // origin (for an existing instance).
    let orig_id = crate::named::named_origin(arena, typ);
    let tparams_len = match arena.get(orig_id) {
        TypeData::Named(n) => n.tparams.as_ref().map_or(0, |l| l.len()),
        _ => 0,
    };
    if tparams_len == 0 {
        return typ; // non-generic — no substitution to do
    }

    let existing_targs = crate::named::named_type_args(arena, typ)
        .map(|l| l.list().to_vec())
        .unwrap_or_default();
    if existing_targs.is_empty() {
        // Declared Named with type params but no targs — nothing to do
        // (uninstantiated generic stays uninstantiated; callers should
        // call `instance` explicitly if they want to apply targs).
        return typ;
    }
    if existing_targs.len() != tparams_len {
        return typ; // mismatch — error reported elsewhere
    }
    let mut changed = false;
    let mut new_targs = Vec::with_capacity(existing_targs.len());
    for ta in &existing_targs {
        let new_ta = subst(arena, oarena, smap, expanding, ctxt, *ta);
        if new_ta != *ta {
            changed = true;
        }
        new_targs.push(new_ta);
    }
    if !changed {
        return typ;
    }
    crate::instantiate::new_named_instance(arena, oarena, ctxt, orig_id, new_targs)
}

// ----------------------------------------------------------------------------
// Helpers for Var / Func substitution.

fn subst_var(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    smap: &SubstMap,
    expanding: Option<TypeId>,
    ctxt: &mut Context,
    v: ObjectId,
) -> ObjectId {
    let v_typ = v.typ(oarena).expect("Var has typ");
    let new_typ = subst(arena, oarena, smap, expanding, ctxt, v_typ);
    if new_typ == v_typ {
        return v;
    }
    clone_var_with_type(oarena, v, new_typ)
}

fn subst_func(
    arena: &mut TypeArena,
    oarena: &mut ObjectArena,
    smap: &SubstMap,
    expanding: Option<TypeId>,
    ctxt: &mut Context,
    f: ObjectId,
) -> ObjectId {
    let f_typ = match f.typ(oarena) {
        Some(t) => t,
        None => return f,
    };
    let new_typ = subst(arena, oarena, smap, expanding, ctxt, f_typ);
    if new_typ == f_typ {
        return f;
    }
    clone_func_with_type(oarena, f, new_typ)
}

/// Allocate a new Var with the same fields as `v` but typed `new_typ`.
/// Mirrors Go's `cloneVar`.
fn clone_var_with_type(oarena: &mut ObjectArena, v: ObjectId, new_typ: TypeId) -> ObjectId {
    let (name, kind, embedded) = match oarena.get(v) {
        ObjectData::Var(orig) => (orig.name().to_string(), orig.kind(), orig.embedded()),
        _ => panic!("clone_var_with_type: expected Var"),
    };
    let id = if embedded {
        crate::object::var::new_field(oarena, name, new_typ, true)
    } else {
        let made = crate::object::var::new_var(oarena, name, new_typ);
        // Carry over kind (new_var defaults to Package).
        if let ObjectData::Var(v) = oarena.get_mut(made) {
            v.set_kind(kind);
        }
        made
    };
    // Carry over package binding.
    if let Some(pkg) = v.pkg(oarena) {
        id.set_pkg(oarena, pkg);
    }
    id
}

/// Allocate a new Func with the same fields as `f` but typed `new_typ`.
/// Mirrors Go's `cloneFunc`.
fn clone_func_with_type(oarena: &mut ObjectArena, f: ObjectId, new_typ: TypeId) -> ObjectId {
    let name = match oarena.get(f) {
        ObjectData::Func(func) => func.name().to_string(),
        _ => panic!("clone_func_with_type: expected Func"),
    };
    let id = crate::object::func::new_func(oarena, name, Some(new_typ));
    if let Some(pkg) = f.pkg(oarena) {
        id.set_pkg(oarena, pkg);
    }
    id
}
