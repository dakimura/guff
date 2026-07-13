//! Port of `cmd/compile/internal/types2/infer.go`.
//!
//! Type-argument inference at generic call sites: given a list of type
//! parameters `tparams`, a possibly-partial list of type arguments `targs`,
//! a parameter tuple `params`, and a list of argument types `args`, attempt
//! to infer a complete `targs` list.
//!
//! ## Decoupling from `Checker`
//!
//! Go's `Checker.infer` uses Checker for:
//! - error reporting (`err.addf`),
//! - `check.subst` / `check.context()` for substitution,
//! - `check.allowVersion(go1_21)` to enable interface-inference unify mode,
//! - `check.hasAllMethods` for constraint method-set check.
//!
//! In our port:
//! - **Errors** are returned as `Option<Vec<TypeId>>` (None = inference
//!   failed). No error message is produced — the Checker chunk will wrap
//!   this with proper reporting.
//! - **Subst** uses our [`crate::subst::subst`] with a fresh `Context`.
//! - **`allow_version`** is a `bool` parameter (caller decides Go 1.21+).
//!   Since chunk 12's Unifier panics on `enable_interface_inference=true`,
//!   we currently pass `false` regardless. Carrying the bool keeps the
//!   API forward-compatible.
//! - **`hasAllMethods` step** is omitted — it's a chunk-11 deferral and
//!   leaves a TODO comment.
//!
//! ## Operand-dependent deferrals
//!
//! Go's `args []*operand` carries `mode`, `typ`, `expr`, `val`, `isNil()`.
//! Until `operand.go` is ported (Tier 3), [`infer`] takes a simpler
//! `&[Option<TypeId>]` — `None` means "skip / unknown / untyped". The
//! untyped-argument default-type step (Go's `--- 3 ---`) is deferred; pass
//! all-typed args for now.

use std::collections::{HashMap, HashSet};

use crate::alias::unalias_readonly;
use crate::arena::{ObjectArena, ObjectId, PackageArena, TypeArena, TypeData, TypeId};
use crate::context::Context;
use crate::interface::interface_compute_typeset;
use crate::predicates::{default_type, max_type};
use crate::subst::{make_subst_map, subst, SubstMap};
use crate::typelists::TypeParamList;
use crate::typeparam::type_param_iface;
use crate::typeset::TypeSet;
use crate::under::common_under;
use crate::unify::{unify, Unifier, UnifyMode};

// ============================================================================
// isParameterized

/// Reports whether `typ` contains any of the type parameters in `tparams`.
///
/// For Signature types, skips its own `tparams`/`rparams` declarations and
/// only looks at the input/result parameter types (matches Go).
///
/// Equivalent to `isParameterized`.
pub fn is_parameterized(
    type_arena: &TypeArena,
    object_arena: &ObjectArena,
    tparams: &[TypeId],
    typ: TypeId,
) -> bool {
    let mut w = TpWalker {
        tparams,
        seen: HashMap::new(),
    };
    w.is_parameterized(type_arena, object_arena, typ)
}

struct TpWalker<'a> {
    tparams: &'a [TypeId],
    seen: HashMap<TypeId, bool>,
}

impl<'a> TpWalker<'a> {
    fn is_parameterized(
        &mut self,
        type_arena: &TypeArena,
        object_arena: &ObjectArena,
        typ: TypeId,
    ) -> bool {
        if let Some(&x) = self.seen.get(&typ) {
            return x;
        }
        // Tentative "false" while we recurse — matches Go's defer-update.
        self.seen.insert(typ, false);
        let res = self.compute(type_arena, object_arena, typ);
        self.seen.insert(typ, res);
        res
    }

    fn compute(&mut self, type_arena: &TypeArena, object_arena: &ObjectArena, typ: TypeId) -> bool {
        match type_arena.get(typ) {
            TypeData::Basic(_) => false,
            TypeData::Alias(_) => {
                let u = unalias_readonly(type_arena, typ);
                if u == typ {
                    false
                } else {
                    self.is_parameterized(type_arena, object_arena, u)
                }
            }
            TypeData::Array(a) => self.is_parameterized(type_arena, object_arena, a.elem()),
            TypeData::Slice(s) => self.is_parameterized(type_arena, object_arena, s.elem()),
            TypeData::Struct(_) => {
                let fields: Vec<ObjectId> = match type_arena.get(typ) {
                    TypeData::Struct(s) => (0..s.num_fields()).map(|i| s.field(i)).collect(),
                    _ => unreachable!(),
                };
                self.var_list(type_arena, object_arena, &fields)
            }
            TypeData::Pointer(p) => self.is_parameterized(type_arena, object_arena, p.elem()),
            TypeData::Tuple(_) => {
                let vars: Vec<ObjectId> = match type_arena.get(typ) {
                    TypeData::Tuple(t) => (0..t.len()).map(|i| t.at(i)).collect(),
                    _ => unreachable!(),
                };
                self.var_list(type_arena, object_arena, &vars)
            }
            TypeData::Signature(sig) => {
                let params = sig.params();
                let results = sig.results();
                let p_check = match params {
                    Some(p) => self.is_parameterized(type_arena, object_arena, p),
                    None => false,
                };
                if p_check {
                    return true;
                }
                match results {
                    Some(r) => self.is_parameterized(type_arena, object_arena, r),
                    None => false,
                }
            }
            TypeData::Interface(_) => {
                // We can't compute the typeset here (read-only arena), so
                // approximate by walking the explicit methods + embedded
                // types. This is conservative — if a tparam appears inside
                // a typeset.terms-only construct (rare), we'll miss it.
                // The Checker would call this after typesets are computed.
                let (methods, embeds) = match type_arena.get(typ) {
                    TypeData::Interface(i) => (i.methods.clone(), i.embeddeds.clone()),
                    _ => unreachable!(),
                };
                for m in methods {
                    if let Some(mt) = m.typ(object_arena) {
                        if self.is_parameterized(type_arena, object_arena, mt) {
                            return true;
                        }
                    }
                }
                for e in embeds {
                    if self.is_parameterized(type_arena, object_arena, e) {
                        return true;
                    }
                }
                false
            }
            TypeData::Union(_) => {
                let terms: Vec<TypeId> = match type_arena.get(typ) {
                    TypeData::Union(u) => (0..u.len()).map(|i| u.term(i).typ()).collect(),
                    _ => unreachable!(),
                };
                for t in terms {
                    if self.is_parameterized(type_arena, object_arena, t) {
                        return true;
                    }
                }
                false
            }
            TypeData::Map(m) => {
                let k = m.key();
                let v = m.elem();
                self.is_parameterized(type_arena, object_arena, k)
                    || self.is_parameterized(type_arena, object_arena, v)
            }
            TypeData::Chan(c) => self.is_parameterized(type_arena, object_arena, c.elem()),
            TypeData::Named(_) => {
                let args: Vec<TypeId> = crate::named::named_type_args(type_arena, typ)
                    .map(|l| l.list().to_vec())
                    .unwrap_or_default();
                for a in args {
                    if self.is_parameterized(type_arena, object_arena, a) {
                        return true;
                    }
                }
                false
            }
            TypeData::TypeParam(_) => self.tparams.contains(&typ),
        }
    }

    fn var_list(
        &mut self,
        type_arena: &TypeArena,
        object_arena: &ObjectArena,
        vars: &[ObjectId],
    ) -> bool {
        for v in vars {
            let t = v.typ(object_arena).expect("Var has typ");
            if self.is_parameterized(type_arena, object_arena, t) {
                return true;
            }
        }
        false
    }
}

// ============================================================================
// coreTerm

/// Result of [`core_term`]: the term itself, plus the `single` flag.
#[derive(Debug, Clone, Copy)]
pub struct CoreTerm {
    pub tilde: bool,
    pub typ: TypeId,
    pub single: bool,
}

/// If the TypeParam `tpar` has a single specific type `S`, returns
/// `Some((S, single=true))`. Otherwise, if `tpar` has a common-underlying
/// core type, returns `Some((core, single=false))` with `tilde` set if any
/// constraint term had a tilde. Otherwise returns `None`.
///
/// Equivalent to `coreTerm`.
pub fn core_term(
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    tpar: TypeId,
) -> Option<CoreTerm> {
    // Walk the constraint's typeset.
    let iface = type_param_iface(type_arena, object_arena, package_arena, tpar);
    interface_compute_typeset(type_arena, object_arena, package_arena, iface);
    let snapshot: TypeSet = match type_arena.get(iface) {
        TypeData::Interface(i) => i.tset.as_ref().expect("computed above").clone(),
        _ => unreachable!(),
    };

    let mut n = 0usize;
    let mut single_typ: Option<TypeId> = None;
    let mut single_tilde = false;
    let mut any_tilde = false;
    snapshot.is(|tilde, typ| {
        if typ.is_some() {
            n += 1;
            if let Some(ty) = typ {
                single_typ = Some(ty);
                single_tilde = tilde;
            }
            if tilde {
                any_tilde = true;
            }
        }
        true
    });

    if n == 1 {
        if let Some(ty) = single_typ {
            return Some(CoreTerm {
                tilde: single_tilde,
                typ: ty,
                single: true,
            });
        }
    }
    let (cu, _err) = common_under(type_arena, object_arena, package_arena, tpar, None);
    cu.map(|typ| CoreTerm {
        tilde: any_tilde,
        typ,
        single: false,
    })
}

// ============================================================================
// killCycles

/// Detect cycles in `inferred` where a TypeParam's inferred type refers
/// back to that TypeParam (directly or transitively). Sets the cyclic
/// entries to `None`.
///
/// Equivalent to `killCycles`.
pub fn kill_cycles(
    type_arena: &TypeArena,
    object_arena: &ObjectArena,
    tparams: &[TypeId],
    inferred: &mut Vec<Option<TypeId>>,
) {
    let mut w = CycleFinder {
        tparams,
        inferred,
        seen: HashSet::new(),
    };
    for &t in tparams {
        w.walk(type_arena, object_arena, t);
    }
}

struct CycleFinder<'a> {
    tparams: &'a [TypeId],
    inferred: &'a mut Vec<Option<TypeId>>,
    seen: HashSet<TypeId>,
}

impl<'a> CycleFinder<'a> {
    fn walk(&mut self, type_arena: &TypeArena, object_arena: &ObjectArena, typ: TypeId) {
        let typ = unalias_readonly(type_arena, typ);
        if self.seen.contains(&typ) {
            if let TypeData::TypeParam(_) = type_arena.get(typ) {
                if let Some(i) = self.tparams.iter().position(|t| *t == typ) {
                    self.inferred[i] = None;
                }
            }
            return;
        }
        self.seen.insert(typ);
        match type_arena.get(typ) {
            TypeData::Basic(_) => {}
            TypeData::Array(a) => self.walk(type_arena, object_arena, a.elem()),
            TypeData::Slice(s) => self.walk(type_arena, object_arena, s.elem()),
            TypeData::Struct(_) => {
                let fields: Vec<ObjectId> = match type_arena.get(typ) {
                    TypeData::Struct(s) => (0..s.num_fields()).map(|i| s.field(i)).collect(),
                    _ => unreachable!(),
                };
                self.var_list(type_arena, object_arena, &fields);
            }
            TypeData::Pointer(p) => self.walk(type_arena, object_arena, p.elem()),
            TypeData::Signature(_) => {
                let (params, results) = match type_arena.get(typ) {
                    TypeData::Signature(s) => (s.params(), s.results()),
                    _ => unreachable!(),
                };
                if let Some(p) = params {
                    let vars: Vec<ObjectId> = match type_arena.get(p) {
                        TypeData::Tuple(t) => (0..t.len()).map(|i| t.at(i)).collect(),
                        _ => unreachable!(),
                    };
                    self.var_list(type_arena, object_arena, &vars);
                }
                if let Some(r) = results {
                    let vars: Vec<ObjectId> = match type_arena.get(r) {
                        TypeData::Tuple(t) => (0..t.len()).map(|i| t.at(i)).collect(),
                        _ => unreachable!(),
                    };
                    self.var_list(type_arena, object_arena, &vars);
                }
            }
            TypeData::Union(_) => {
                let terms: Vec<TypeId> = match type_arena.get(typ) {
                    TypeData::Union(u) => (0..u.len()).map(|i| u.term(i).typ()).collect(),
                    _ => unreachable!(),
                };
                for t in terms {
                    self.walk(type_arena, object_arena, t);
                }
            }
            TypeData::Interface(_) => {
                let (methods, embeds) = match type_arena.get(typ) {
                    TypeData::Interface(i) => (i.methods.clone(), i.embeddeds.clone()),
                    _ => unreachable!(),
                };
                for m in methods {
                    if let Some(mt) = m.typ(object_arena) {
                        self.walk(type_arena, object_arena, mt);
                    }
                }
                for e in embeds {
                    self.walk(type_arena, object_arena, e);
                }
            }
            TypeData::Map(m) => {
                let k = m.key();
                let v = m.elem();
                self.walk(type_arena, object_arena, k);
                self.walk(type_arena, object_arena, v);
            }
            TypeData::Chan(c) => self.walk(type_arena, object_arena, c.elem()),
            TypeData::Named(_) => {
                let args: Vec<TypeId> = crate::named::named_type_args(type_arena, typ)
                    .map(|l| l.list().to_vec())
                    .unwrap_or_default();
                for a in args {
                    self.walk(type_arena, object_arena, a);
                }
            }
            TypeData::TypeParam(_) => {
                if let Some(i) = self.tparams.iter().position(|t| *t == typ) {
                    if let Some(inf) = self.inferred[i] {
                        self.walk(type_arena, object_arena, inf);
                    }
                }
            }
            TypeData::Tuple(_) | TypeData::Alias(_) => {
                // Tuples don't appear at the top level here (handled
                // via Signature above). Aliases are unaliased at entry.
            }
        }
        self.seen.remove(&typ);
    }

    fn var_list(&mut self, type_arena: &TypeArena, object_arena: &ObjectArena, vars: &[ObjectId]) {
        for v in vars {
            let t = v.typ(object_arena).expect("Var has typ");
            self.walk(type_arena, object_arena, t);
        }
    }
}

// ============================================================================
// renameTParams

/// Rename the TypeParams `tparams` so each has a fresh identity, and
/// substitute `typ` accordingly. Returns the new tparam list and the new
/// type. If `typ` doesn't contain any of `tparams`, the returned type
/// equals `typ` (unchanged).
///
/// Equivalent to `Checker.renameTParams`. The chunk-13 port creates the
/// fresh `TypeName`s with `pkg = None` and `pos = 0` since position
/// fidelity isn't carried yet.
pub fn rename_tparams(
    type_arena: &mut TypeArena,
    object_arena: &mut ObjectArena,
    tparams: &[TypeId],
    typ: TypeId,
) -> (Vec<TypeId>, TypeId) {
    if tparams.is_empty() {
        return (Vec::new(), typ);
    }
    // Allocate fresh TypeNames + TypeParams.
    let mut new_tparams = Vec::with_capacity(tparams.len());
    for (i, tp) in tparams.iter().enumerate() {
        let (name, orig_idx) = match type_arena.get(*tp) {
            TypeData::TypeParam(tparam) => {
                let n = tparam.obj().name(object_arena).to_string();
                (n, tparam.index())
            }
            _ => panic!("rename_tparams: tparam #{} is not a TypeParam", i),
        };
        let tname = crate::object::type_name::new_type_name(object_arena, name, None);
        let new_tp = crate::typeparam::new_type_param(type_arena, tname, None);
        // Set index directly (we don't call bind_tparams to avoid the
        // already-bound check, since the original tparam carries
        // index >= 0).
        if let TypeData::TypeParam(t) = type_arena.get_mut(new_tp) {
            t.set_index(orig_idx);
        }
        crate::object::type_name::type_name_set_typ(object_arena, tname, new_tp);
        new_tparams.push(new_tp);
    }

    // Build the rename map and substitute the bounds + typ.
    let smap: SubstMap = make_subst_map(tparams, &new_tparams);
    for (i, &tp) in tparams.iter().enumerate() {
        let bound = match type_arena.get(tp) {
            TypeData::TypeParam(t) => t.constraint(),
            _ => unreachable!(),
        };
        if let Some(b) = bound {
            let mut ctxt = Context::new();
            let new_b = subst(type_arena, object_arena, &smap, None, &mut ctxt, b);
            crate::typeparam::set_constraint(type_arena, new_tparams[i], new_b);
        }
    }
    let mut ctxt = Context::new();
    let new_typ = subst(type_arena, object_arena, &smap, None, &mut ctxt, typ);
    (new_tparams, new_typ)
}

// ============================================================================
// infer — the main entry point

/// Outcome of [`infer`].
#[derive(Debug, Clone)]
pub enum InferResult {
    /// Full inference succeeded — `targs[i]` is the inferred type for
    /// `tparams[i]`.
    Ok(Vec<TypeId>),
    /// One or more type parameters could not be inferred. The carried
    /// list has `Some` for inferred entries and `None` for failures.
    Failed(Vec<Option<TypeId>>),
}

/// Attempt to infer type arguments for `tparams` from `params` and `args`.
///
/// - `tparams`: list of TypeParams to infer (must be non-empty).
/// - `targs`: partial type arguments — `Some` to pre-bind, `None` for "infer
///   me". May be shorter than `tparams` (treated as `None`-padded).
/// - `params`: parameter Tuple TypeId (`None` for empty parameter list).
/// - `args`: argument types — `Some(t)` for typed args, `None` for "skip"
///   (untyped or invalid). Length must equal the parameter count when
///   `params.is_some()`.
/// - `enable_interface_inference`: matches Go's `check.allowVersion(go1_21)`.
///   Currently must be `false` (chunk-12 unify deferral panics otherwise).
///
/// Returns [`InferResult::Ok`] on full success, [`InferResult::Failed`] if
/// any tparam remains unresolved or a cycle is detected.
///
/// **Operand-dependent deferrals** (carry into Tier-3 port):
/// - untyped-argument default-type promotion (Go's `--- 3 ---` block).
/// - `hasAllMethods` constraint method-set check (chunk-11 deferral).
/// - `reverse` flag for reverse-inference error formatting.
///
/// Equivalent to `Checker.infer` minus the deferrals.
pub fn infer(
    type_arena: &mut TypeArena,
    object_arena: &mut ObjectArena,
    package_arena: &PackageArena,
    tparams: &[TypeId],
    targs: &[Option<TypeId>],
    params: Option<TypeId>,
    args: &[Option<TypeId>],
    untyped_args: &[Option<TypeId>],
    typ_table: &[TypeId],
    enable_interface_inference: bool,
) -> InferResult {
    let n = tparams.len();
    assert!(n > 0);
    assert!(targs.len() <= n);

    // Param/arg count must match (zero allowed).
    let params_len = params
        .map(|p| match type_arena.get(p) {
            TypeData::Tuple(t) => t.len(),
            _ => 0,
        })
        .unwrap_or(0);
    assert_eq!(params_len, args.len());

    // Fast path: full targs already provided with no None entries.
    if targs.len() == n && targs.iter().all(|t| t.is_some()) {
        return InferResult::Ok(targs.iter().map(|t| t.unwrap()).collect());
    }

    // Pad targs to length n.
    let mut targs_padded: Vec<Option<TypeId>> = targs.to_vec();
    targs_padded.resize(n, None);

    // Substitute provided targs into params for better matching. We skip
    // this step if there are no provided targs (no-op anyway), matching
    // Go's optimization.
    let params = if params_len > 0 && targs_padded.iter().any(|t| t.is_some()) {
        // Build a SubstMap with only the Some entries (chunk-9 subst
        // skips lookups it doesn't have).
        let mut smap: SubstMap = HashMap::new();
        for (i, t) in targs_padded.iter().enumerate() {
            if let Some(t) = t {
                smap.insert(tparams[i], *t);
            }
        }
        let mut ctxt = Context::new();
        params.map(|p| subst(type_arena, object_arena, &smap, None, &mut ctxt, p))
    } else {
        params
    };

    let mut u = Unifier::new(tparams, &targs_padded, enable_interface_inference);

    // --- 1 --- use information from function arguments.
    for (i, arg_opt) in args.iter().enumerate() {
        let arg = match arg_opt {
            Some(a) => *a,
            None => continue, // skip / untyped (deferred)
        };
        let par_typ = {
            let p = params.expect("params must be Some when args is non-empty");
            let var = match type_arena.get(p) {
                TypeData::Tuple(t) => t.at(i),
                _ => unreachable!(),
            };
            var.typ(object_arena).expect("param Var has typ")
        };
        let par_is_param = is_parameterized(type_arena, object_arena, tparams, par_typ);
        let arg_is_param = is_parameterized(type_arena, object_arena, tparams, arg);
        if par_is_param || arg_is_param {
            if !unify(
                &mut u,
                type_arena,
                object_arena,
                package_arena,
                par_typ,
                arg,
                UnifyMode::ASSIGN,
            ) {
                return InferResult::Failed(u.inferred(tparams));
            }
        }
    }

    // --- 2 --- use information from type-parameter constraints. Loop
    // until no progress.
    loop {
        let unknowns_before = u.unknowns();
        for &tpar in tparams {
            let tx = u.at(tpar);
            let core = core_term(type_arena, object_arena, package_arena, tpar);
            if let Some(c) = core {
                match (tx, c.single) {
                    (Some(t), _) => {
                        if !unify(
                            &mut u,
                            type_arena,
                            object_arena,
                            package_arena,
                            t,
                            c.typ,
                            UnifyMode::ZERO,
                        ) {
                            return InferResult::Failed(u.inferred(tparams));
                        }
                    }
                    (None, true) if !c.tilde => {
                        u.set(tpar, c.typ);
                    }
                    _ => {}
                }
            }
            // TODO(chunk-N): hasAllMethods constraint check — needs the
            // chunk-11 deferral lifted.
        }
        if u.unknowns() == unknowns_before {
            break;
        }
    }

    // --- 3 --- untyped-argument default-type promotion.
    //
    // `untyped_args[i]` is `Some(t)` when argument `i` was an untyped, non-nil
    // constant whose (untyped) type is `t`. Such arguments were withheld from
    // step 1 (passed as `None` in `args`) because an untyped value can only
    // match a single type parameter — never a composite parameter type. Some of
    // those type parameters may already have a type by now; for the rest, take
    // the maximum untyped type across all untyped arguments and set the
    // parameter to that type's default. Mirrors `infer.go`'s step 3.
    if !untyped_args.is_empty() {
        // (type-parameter id, maximum untyped type seen so far)
        let mut max_untyped: Vec<(TypeId, TypeId)> = Vec::new();
        for i in 0..args.len() {
            let ut = match untyped_args.get(i).copied().flatten() {
                Some(t) => t,
                None => continue,
            };
            // The parameter at i must be a single type parameter (by
            // construction of the untyped list). If a provided targ already
            // substituted it to a concrete type, it is no longer a TypeParam
            // and we skip it.
            let p = match params {
                Some(p) => p,
                None => continue,
            };
            let par_typ = match type_arena.get(p) {
                TypeData::Tuple(t) => match t.at(i).typ(object_arena) {
                    Some(t) => t,
                    None => continue,
                },
                _ => continue,
            };
            if !matches!(type_arena.get(par_typ), TypeData::TypeParam(_)) {
                continue;
            }
            let tpar = par_typ;
            if u.at(tpar).is_some() {
                continue; // already inferred in steps 1-2
            }
            // Accumulate the maximum untyped type for this type parameter.
            if let Some(slot) = max_untyped.iter_mut().find(|(t, _)| *t == tpar) {
                match max_type(type_arena, slot.1, ut) {
                    Some(m) => slot.1 = m,
                    None => return InferResult::Failed(u.inferred(tparams)),
                }
            } else {
                max_untyped.push((tpar, ut));
            }
        }
        for (tpar, typ) in max_untyped {
            let d = default_type(type_arena, typ_table, typ);
            u.set(tpar, d);
        }
    }

    // --- simplify --- repeatedly substitute inferred types into themselves
    // until nothing changes.
    let mut inferred = u.inferred(tparams);
    kill_cycles(type_arena, object_arena, tparams, &mut inferred);

    let mut dirty: Vec<usize> = (0..n)
        .filter(|&i| inferred[i].is_some() && targs_padded.get(i).copied().flatten().is_none())
        .collect();

    while !dirty.is_empty() {
        let smap = build_subst_map(tparams, &inferred);
        let mut still_dirty = Vec::new();
        for &index in &dirty {
            let t0 = inferred[index].expect("dirty entries are Some");
            let mut ctxt = Context::new();
            let t1 = subst(type_arena, object_arena, &smap, None, &mut ctxt, t0);
            if t1 != t0 {
                inferred[index] = Some(t1);
                still_dirty.push(index);
            }
        }
        if still_dirty.len() == dirty.len() {
            // No progress — stop to avoid infinite loop (additional safety
            // beyond Go's because we don't track changes per substitution
            // step).
            let _ = smap;
            break;
        }
        dirty = still_dirty;
    }

    // Final check: any TypeParam still uninferred or still parameterized
    // counts as failure.
    let mut all_ok = true;
    for (i, t_opt) in inferred.iter().enumerate() {
        match t_opt {
            None => {
                all_ok = false;
                let _ = i;
            }
            Some(t) => {
                if is_parameterized(type_arena, object_arena, tparams, *t) {
                    all_ok = false;
                }
            }
        }
    }
    if all_ok {
        InferResult::Ok(inferred.into_iter().map(|o| o.unwrap()).collect())
    } else {
        InferResult::Failed(inferred)
    }
}

/// Build a SubstMap from `tparams` and a parallel `inferred` list,
/// skipping `None` entries.
fn build_subst_map(tparams: &[TypeId], inferred: &[Option<TypeId>]) -> SubstMap {
    let mut m: SubstMap = HashMap::with_capacity(tparams.len());
    for (i, &tp) in tparams.iter().enumerate() {
        if let Some(t) = inferred[i] {
            m.insert(tp, t);
        }
    }
    m
}

// Silence unused-import in case TypeParamList isn't referenced in
// downstream cfg builds.
#[allow(dead_code)]
fn _unused(_: TypeParamList) {}
