//! Checker-side interface-satisfaction logic, ported from `lookup.go`
//! (`missingMethod`, `hasAllMethods`) and `instantiate.go` (`implements`).
//!
//! This is the chunk-20a recovery of the chunk-11 deferral: the structural
//! `lookup.rs` is done, and now the `Checker` gets the real
//! [`implements`] / [`missing_method`].
//!
//! ## Arena-based free functions (chunk 20c)
//!
//! Both routines depend only on the three arenas — not on any other `Checker`
//! state — so they're written as free functions over `(&mut TypeArena,
//! &ObjectArena, &PackageArena, …)`. [`Checker::implements`] etc. are thin
//! delegations. This shape lets `assignments.rs` / `conversions.rs` call the
//! satisfaction logic from inside a closure that the free `assignable_to`
//! threads its own arena into (chunk 20c) — a Checker method capturing `self`
//! couldn't, because `self.types` is already borrowed `&mut`.
//!
//! ## Deferrals
//!
//! - `funcString`'s rich rendering is simplified to a `type_string` of the
//!   signature.
//! - The comparability **version gate** in `implements` (go1.20) is treated as
//!   always satisfied (matches the Checker-less path in `conversions.rs`).
//! - The "possibly missing ~" alternative-type suggestion is omitted.
//!
//! Checker wrappers call [`Checker::ensure_method_sigs`] before the free
//! [`missing_method`] / [`implements`] so package-level `var _ Iface = (*T)(nil)`
//! checks that appear *before* method declarations still see resolved
//! signatures (Go's `objDecl(f)` in `missingMethod`).

use crate::hash::HashSet;

use guff_types_errors::Code;

use crate::arena::{ObjectArena, ObjectData, ObjectId, PackageArena, TypeArena, TypeData, TypeId};
use crate::check::Checker;
use crate::interface::interface_typeset;
use crate::lookup::{
    as_named, deref, has_invalid_embedded_fields, lookup_field_or_method,
    lookup_field_or_method_fold, LookupResult,
};
use crate::named::{named_method, named_num_methods};
use crate::operand::Operand;
use crate::pointer::pointer_elem;
use crate::predicates::{comparable_type, identical, is_interface, is_valid};
use crate::termlist;
use crate::typestring::type_string;

/// The outcome of a failed [`missing_method`] check: which method on `T` was
/// the problem, whether it was a signature/receiver mismatch (vs. a plain
/// absence), and a human-readable cause.
///
/// Equivalent to Go's `(method *Func, wrongType bool)` plus the `*cause`
/// out-param.
#[derive(Debug, Clone)]
pub struct MissingMethod {
    /// The method declared on `T` that `V` fails to provide correctly.
    pub method: ObjectId,
    /// True if a method *was* found but with the wrong signature or a pointer
    /// receiver — Go's `wrongType`.
    pub wrong_type: bool,
    /// Human-readable explanation (parenthesised, like Go's `*cause`).
    pub cause: String,
}

// State of the method search (mirrors Go's local `iota` consts).
#[derive(Copy, Clone, PartialEq, Eq)]
enum State {
    Ok,
    NotFound,
    WrongName,
    Unexported,
    WrongSig,
    AmbigSel,
    PtrRecv,
    Field,
}

/// Cast `obj` to a method (`*Func`), or `None` if it's not a function.
fn as_func(oarena: &ObjectArena, obj: ObjectId) -> Option<ObjectId> {
    match oarena.get(obj) {
        ObjectData::Func(_) => Some(obj),
        _ => None,
    }
}

/// Compare two (optional) signature types with `Identical`. Missing types
/// never match.
fn sig_identical(
    types: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    a: Option<TypeId>,
    b: Option<TypeId>,
) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => identical(types, oarena, parena, a, b),
        _ => false,
    }
}

/// Report which method (if any) of interface `t` is missing from, or wrongly
/// implemented by, `v`. Returns `None` when `v` provides every method of `t`.
///
/// Equivalent to `Checker.missingMethod` (with `equivalent = Identical`).
pub fn missing_method(
    types: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    v: TypeId,
    t: TypeId,
    static_: bool,
) -> Option<MissingMethod> {
    let tu = t.underlying(types);
    let methods = match types.get(tu) {
        TypeData::Interface(_) => {
            let ts = interface_typeset(types, oarena, parena, tu);
            ts.methods().to_vec()
        }
        _ => return None,
    };
    if methods.is_empty() {
        return None;
    }

    let vu = v.underlying(types);
    let v_iface = matches!(types.get(vu), TypeData::Interface(_));

    let mut state = State::Ok;
    let mut cur_m: Option<ObjectId> = None; // method on T being matched
    let mut cur_f: Option<ObjectId> = None; // method found on V (if any)

    if v_iface {
        let vts = interface_typeset(types, oarena, parena, vu);
        for &m in &methods {
            cur_m = Some(m);
            let name = m.name(oarena).to_string();
            let pkg = m.pkg(oarena);
            match vts.lookup_method(oarena, parena, pkg, &name, false) {
                None => {
                    if !static_ {
                        continue;
                    }
                    state = State::NotFound;
                    break;
                }
                Some((_, f)) => {
                    cur_f = Some(f);
                    let ftyp = f.typ(oarena);
                    let mtyp = m.typ(oarena);
                    if !sig_identical(types, oarena, parena, ftyp, mtyp) {
                        state = State::WrongSig;
                        break;
                    }
                }
            }
        }
    } else {
        for &m in &methods {
            cur_m = Some(m);
            let name = m.name(oarena).to_string();
            let pkg = m.pkg(oarena);
            let res = lookup_field_or_method(types, oarena, parena, v, false, pkg, &name);
            match res {
                LookupResult::Ambiguous { .. } => {
                    state = State::AmbigSel;
                    break;
                }
                LookupResult::PtrRecvRequired => {
                    state = State::PtrRecv;
                    break;
                }
                LookupResult::NotFound => {
                    state = State::NotFound;
                    // Retry case-insensitively for a wrong-name / unexported
                    // candidate (better error message).
                    let fold = lookup_field_or_method_fold(
                        types, oarena, parena, v, false, pkg, &name, true,
                    );
                    if let LookupResult::Found { obj, .. } = fold {
                        if let Some(f) = as_func(oarena, obj) {
                            cur_f = Some(f);
                            state = State::WrongName;
                            if f.name(oarena) == name {
                                state = State::Unexported;
                            }
                        }
                    }
                    break;
                }
                LookupResult::Found { obj, .. } => match as_func(oarena, obj) {
                    None => {
                        state = State::Field;
                        break;
                    }
                    Some(f) => {
                        cur_f = Some(f);
                        // Free-function path cannot call Checker::obj_decl; see
                        // Checker::ensure_method_sigs for the source-checked case.
                        let ftyp = f.typ(oarena);
                        let mtyp = m.typ(oarena);
                        if !sig_identical(types, oarena, parena, ftyp, mtyp) {
                            state = State::WrongSig;
                            break;
                        }
                    }
                },
            }
        }
    }

    if state == State::Ok {
        return None;
    }

    let m = cur_m.expect("non-ok state must have a current method");
    let m_name = m.name(oarena).to_string();
    let sig_str = |types: &TypeArena, obj: Option<ObjectId>| -> String {
        obj.and_then(|o| o.typ(oarena))
            .map(|s| type_string(types, oarena, parena, s, None))
            .unwrap_or_default()
    };
    let cause = match state {
        State::NotFound => format!("(missing method {})", m_name),
        State::WrongName => format!(
            "(missing method {})\n\t\thave {}\n\t\twant {}",
            m_name,
            sig_str(types, cur_f),
            sig_str(types, Some(m))
        ),
        State::Unexported => format!("(unexported method {})", m_name),
        State::WrongSig => format!(
            "(wrong type for method {})\n\t\thave {}\n\t\twant {}",
            m_name,
            sig_str(types, cur_f),
            sig_str(types, Some(m))
        ),
        State::AmbigSel => format!("(ambiguous selector {})", m_name),
        State::PtrRecv => format!("(method {} has pointer receiver)", m_name),
        State::Field => format!("({} is a field, not a method)", m_name),
        State::Ok => unreachable!(),
    };

    Some(MissingMethod {
        method: m,
        wrong_type: matches!(state, State::WrongSig | State::PtrRecv),
        cause,
    })
}

/// Reports whether `v` implements (or, for `constraint`, satisfies) the
/// interface `t`. `Ok(())` on success, `Err(cause)` otherwise.
///
/// Equivalent to `Checker.implements` (`instantiate.go`). The go1.20
/// comparability version gate and the "possibly missing ~" suggestion are
/// deferred (see module docs).
pub fn implements(
    types: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    v: TypeId,
    t: TypeId,
    constraint: bool,
) -> Result<(), String> {
    let ts = |types: &TypeArena, x: TypeId| type_string(types, oarena, parena, x, None);

    let vu = v.underlying(types);
    let tu = t.underlying(types);
    if !is_valid(types, vu) || !is_valid(types, tu) {
        return Ok(()); // avoid follow-on errors
    }
    // Pointer to an invalid base: avoid follow-on errors.
    if let TypeData::Pointer(_) = types.get(vu) {
        let base = pointer_elem(types, vu);
        let base_u = base.underlying(types);
        if !is_valid(types, base_u) {
            return Ok(());
        }
    }

    let verb = if constraint { "satisfy" } else { "implement" };

    // T's underlying must be an interface.
    if !matches!(types.get(tu), TypeData::Interface(_)) {
        let detail = format!("{} is not an interface", ts(types, t));
        return Err(format!(
            "{} does not {} {} ({})",
            ts(types, v),
            verb,
            ts(types, t),
            detail
        ));
    }

    let tts = interface_typeset(types, oarena, parena, tu);
    // Every type satisfies the empty interface.
    if tts.is_all() {
        return Ok(());
    }

    // An interface V with an empty type set satisfies any interface.
    let v_iface = matches!(types.get(vu), TypeData::Interface(_));
    let vts = if v_iface {
        Some(interface_typeset(types, oarena, parena, vu))
    } else {
        None
    };
    if let Some(vts) = &vts {
        if vts.is_empty() {
            return Ok(());
        }
    }

    // No type with a non-empty type set satisfies the empty type set.
    if tts.is_empty() {
        return Err(format!("cannot {} {} (empty type set)", verb, ts(types, t)));
    }

    // V must implement T's methods.
    if let Some(mm) = missing_method(types, oarena, parena, v, t, true) {
        return Err(format!(
            "{} does not {} {} {}",
            ts(types, v),
            verb,
            ts(types, t),
            mm.cause
        ));
    }

    // Comparability: if T is comparable, V must be comparable.
    let check_comparability = |types: &mut TypeArena| -> Result<(), String> {
        // `if !Ti.IsComparable() { return true }` — the computed answer, not the
        // `comparable` flag: an interface whose terms are all comparable is
        // comparable even though it never embedded `comparable`.
        let mut tseen = HashSet::default();
        if !crate::predicates::typeset_is_comparable(types, oarena, parena, tu, &mut tseen) {
            return Ok(());
        }
        let mut seen = HashSet::default();
        if comparable_type(types, oarena, parena, v, false, &mut seen).is_ok() {
            return Ok(());
        }
        if constraint {
            let mut seen2 = HashSet::default();
            if comparable_type(types, oarena, parena, v, true, &mut seen2).is_ok() {
                // DEFERRED: go1.20 version gate — treated as satisfied.
                return Ok(());
            }
        }
        Err(format!(
            "{} does not {} comparable",
            type_string(types, oarena, parena, v, None),
            verb
        ))
    };

    // V must also be in T's type set, if T restricts it.
    if !tts.has_terms() {
        return check_comparability(types);
    }

    if let Some(vts) = &vts {
        // V is an interface: its type set must be a subset of T's.
        if !termlist::subset_of(types, oarena, parena, &vts.terms, &tts.terms) {
            return Err(format!(
                "{} does not {} {}",
                ts(types, v),
                verb,
                ts(types, t)
            ));
        }
        return check_comparability(types);
    }

    // Otherwise V's type must be included in T's type set.
    if !termlist::includes(types, oarena, parena, &tts.terms, v) {
        return Err(format!(
            "{} does not {} {}",
            ts(types, v),
            verb,
            ts(types, t)
        ));
    }
    check_comparability(types)
}

impl Checker {
    /// Force [`Self::obj_decl`] on every method attached to `v`'s named base
    /// (following a single pointer indirection). Mirrors Go `missingMethod`'s
    /// `objDecl(f)` so interface checks see real signatures even when a
    /// package-level `var _ I = (T)(nil)` appears *before* the method decls.
    pub fn ensure_method_sigs(&mut self, v: TypeId) {
        let mut seen = crate::hash::HashSet::default();
        self.ensure_method_sigs_rec(v, &mut seen);
    }

    /// `ensure_method_sigs`, following embedded fields.
    ///
    /// A method set is not only the type's own methods: `struct{ Multi }`
    /// answers `Reset()` out of `Multi`, and the lookup that finds it needs
    /// `Multi`'s *signatures* resolved just as much as it needs `Wrap`'s.
    /// Stopping at the outer type left promoted methods unresolved, so
    ///
    /// ```go
    /// var _ Resettable = &Wrap{}   // ← checked here
    /// type Wrap struct{ Multi }
    /// type Multi []Base
    /// func (m Multi) Reset() {}    // ← declared here
    /// ```
    ///
    /// failed while the same file with the `var` moved to the bottom passed —
    /// an order dependence, which is the signature of a missing lazy
    /// completion rather than a missing rule. Reduced from kubernetes'
    /// `apimachinery/pkg/api/meta` (483 files → 3, COMPAT-HARDENING §4).
    fn ensure_method_sigs_rec(&mut self, v: TypeId, seen: &mut crate::hash::HashSet<TypeId>) {
        use crate::named::named_origin;
        use crate::r#struct::{struct_field, struct_num_fields};

        let (base, _) = deref(&self.types, v);
        if !seen.insert(base) {
            return;
        }
        let Some(named) = as_named(&self.types, base) else {
            return;
        };
        // Methods live on the origin for instances.
        let origin = named_origin(&self.types, named);
        let n = named_num_methods(&self.types, origin);
        // Collect ids first — `obj_decl` may mutate arenas.
        let methods: Vec<ObjectId> = (0..n).map(|i| named_method(&self.types, origin, i)).collect();
        for m in methods {
            if matches!(self.objects.get(m), ObjectData::Func(_)) {
                self.obj_decl(m);
            }
        }

        // Descend through embedded fields. Snapshot first: `obj_decl` above and
        // the recursion below both mutate the arenas.
        let u = base.underlying(&self.types);
        if !matches!(self.types.get(u), crate::arena::TypeData::Struct(_)) {
            return;
        }
        let embedded: Vec<TypeId> = (0..struct_num_fields(&self.types, u))
            .map(|i| struct_field(&self.types, u, i))
            .filter(|f| match self.objects.get(*f) {
                ObjectData::Var(var) => var.embedded(),
                _ => false,
            })
            .filter_map(|f| f.typ(&self.objects))
            .collect();
        for e in embedded {
            self.ensure_method_sigs_rec(e, seen);
        }
    }

    /// Resolve `v`'s method signatures and, if `v` is a generic instance,
    /// substitute them — everything the free functions below need in place
    /// before they can compare a method set.
    ///
    /// Go needs no such step: `Named.Method(i)` expands lazily, so every reader
    /// of a method set gets substituted signatures whoever asks first. guff
    /// expands eagerly at a call site, which means **every** entry point into an
    /// interface-satisfaction check has to do it. `assignable_to` did; the
    /// assertion path did not, so `x.(S[T])` compared the *origin's* `add` (bound
    /// to `S`'s own `T`) against the interface's substituted one and called the
    /// assertion impossible. Both print as `func(item T, priority int)`, which is
    /// why the resulting error names the same type on both sides of "does not
    /// implement". Found by reducing controller-runtime's priorityqueue
    /// (COMPAT-HARDENING Phase 6).
    ///
    /// Resolution comes first: `expand_instance_methods` copies the origin's
    /// signatures and then refuses to run twice, so expanding before the origin
    /// is resolved would bake unresolved signatures into the instance for good.
    pub(crate) fn prepare_method_set(&mut self, v: TypeId) {
        self.ensure_method_sigs(v);
        self.expand_instance_methods(v);
    }

    /// See the free [`missing_method`].
    pub fn missing_method(&mut self, v: TypeId, t: TypeId, static_: bool) -> Option<MissingMethod> {
        self.prepare_method_set(v);
        missing_method(
            &mut self.types,
            &self.objects,
            &self.packages,
            v,
            t,
            static_,
        )
    }

    /// See the free [`implements`].
    pub fn implements(&mut self, v: TypeId, t: TypeId, constraint: bool) -> Result<(), String> {
        self.prepare_method_set(v);
        implements(
            &mut self.types,
            &self.objects,
            &self.packages,
            v,
            t,
            constraint,
        )
    }

    /// Boolean wrapper: does `v` implement interface `t` (non-constraint)?
    pub fn implements_bool(&mut self, v: TypeId, t: TypeId) -> bool {
        self.implements(v, t, false).is_ok()
    }

    /// Reports whether every method of `t` is present on `v`. Used to avoid
    /// follow-on errors due to incorrect types.
    ///
    /// Equivalent to `Checker.hasAllMethods` (with `equivalent = Identical`,
    /// which is what `missing_method` already uses). Returns `Ok(())` when all
    /// methods are present, `Err(cause)` otherwise.
    pub fn has_all_methods(&mut self, v: TypeId, t: TypeId, static_: bool) -> Result<(), String> {
        // We don't know anything about an invalid V — assume it implements T.
        if !is_valid(&self.types, v) {
            return Ok(());
        }
        self.prepare_method_set(v);
        match missing_method(
            &mut self.types,
            &self.objects,
            &self.packages,
            v,
            t,
            static_,
        ) {
            None => Ok(()),
            Some(mm) => {
                // An invalid embedded field could hide the method — assume present.
                if has_invalid_embedded_fields(&self.types, &self.objects, v) {
                    Ok(())
                } else {
                    Err(mm.cause)
                }
            }
        }
    }

    /// Populate the method list of the generic *instance* reachable from `v`
    /// (after one optional pointer dereference) with copies of the origin's
    /// methods whose signatures are instantiated with the instance's type
    /// arguments. Idempotent (skips an instance that already has methods) and a
    /// no-op for non-instances.
    ///
    /// This is Go's lazy `Named.Method(i)` / `expandMethod`, driven from the
    /// `Checker` where `&mut` access to all arenas (and the `Context`) is
    /// available. Doing it here — at the point an instance is checked for
    /// interface satisfaction — guarantees the origin's method signatures are
    /// already resolved (unlike expanding eagerly at instantiation time, which
    /// can precede method-signature resolution for struct-field instances).
    /// Once populated, `named_lookup_method` finds the substituted methods
    /// directly, so `missing_method` compares the instantiated signatures.
    pub fn expand_instance_methods(&mut self, v: TypeId) {
        let (base, _) = crate::lookup::deref(&self.types, v);
        let named = match crate::lookup::as_named(&self.types, base) {
            Some(n) => n,
            None => return,
        };
        let targs: Vec<TypeId> = match crate::named::named_type_args(&self.types, named) {
            Some(list) => list.list().to_vec(),
            None => return, // not an instance
        };
        if crate::named::named_num_methods(&self.types, named) > 0 {
            return; // already expanded
        }
        let origin = crate::named::named_origin(&self.types, named);
        let n = crate::named::named_num_methods(&self.types, origin);
        for i in 0..n {
            let om = crate::named::named_method(&self.types, origin, i);
            let expanded = self.expand_one_method(named, om, &targs);
            crate::named::push_method(&mut self.types, named, expanded);
        }
    }

    /// Instantiate origin method `om` for instance `inst` with type arguments
    /// `targs`, substituting the method's receiver type parameters. Returns a
    /// fresh `Func` with the substituted signature, or `om` unchanged when there
    /// is nothing to substitute.
    fn expand_one_method(&mut self, inst: TypeId, om: ObjectId, targs: &[TypeId]) -> ObjectId {
        // Force the origin method's signature to be resolved before reading it,
        // so we never capture an unresolved (placeholder) signature (Go's
        // `Named.resolve` fully resolves the origin first). Matches the
        // `obj_decl` the selector runs on a found method.
        if matches!(self.objects.get(om), ObjectData::Func(_)) {
            self.obj_decl(om);
        }
        let sig = match om.typ(&self.objects) {
            Some(s) => s,
            None => return om,
        };
        let rparams: Vec<TypeId> =
            match crate::signature::signature_recv_type_params(&self.types, sig) {
                Some(list) => list.list().to_vec(),
                None => return om,
            };
        if rparams.is_empty() || rparams.len() != targs.len() {
            return om;
        }
        let smap = crate::subst::make_subst_map(&rparams, targs);
        let new_sig = crate::subst::subst(
            &mut self.types,
            &mut self.objects,
            &smap,
            Some(inst),
            &mut self.ctxt,
            sig,
        );
        let name = om.name(&self.objects).to_string();
        let new_m = crate::object::func::new_func(&mut self.objects, name, Some(new_sig));
        if let Some(pkg) = om.pkg(&self.objects) {
            new_m.set_pkg(&mut self.objects, pkg);
        }
        new_m
    }

    /// Reports whether a value of type `v` can be asserted to have type `t`.
    /// The underlying type of `v` must be an interface.
    ///
    /// Equivalent to `Checker.assertableTo` (`lookup.go`). If `t` is an
    /// interface, no static check is required (the dynamic type is what is
    /// asserted). Otherwise `t` must have all of `v`'s methods.
    pub fn assertable_to(&mut self, v: TypeId, t: TypeId) -> Result<(), String> {
        // No static check required if T is an interface.
        if is_interface(&self.types, t) {
            return Ok(());
        }
        // TODO(gri, upstream): fix this for generalized interfaces.
        self.has_all_methods(t, v, false)
    }

    /// Check the type assertion `x.(T)`. The type of `x` must be an interface.
    ///
    /// Equivalent to `Checker.typeAssertion` (`expr.go`). Reports an
    /// `ImpossibleAssert` error when the assertion can never succeed.
    pub fn type_assertion(&mut self, pos: u32, x: &Operand, t: TypeId, type_switch: bool) {
        let v = x.typ.unwrap_or_else(|| self.invalid_type());
        let cause = match self.assertable_to(v, t) {
            Ok(()) => return, // success
            Err(c) => c,
        };
        let xs = self.operand_str(x);
        if type_switch {
            let ts = self.type_str(t);
            self.error(
                pos,
                Code::ImpossibleAssert,
                format!(
                    "impossible type switch case: {} cannot have dynamic type {} ({})",
                    xs, ts, cause
                ),
            );
            return;
        }
        let vs = self.type_str(v);
        self.error(
            pos,
            Code::ImpossibleAssert,
            format!(
                "impossible type assertion: {} does not implement {} ({})",
                vs, xs, cause
            ),
        );
    }
}
