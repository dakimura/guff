//! Checker wrappers that wire the *real* `implements` / `representable`
//! (chunks 20a/20b) into the structural [`crate::assignments::assignable_to`]
//! and [`crate::conversions::convertible_to`] routines (chunk 20c).
//!
//! The structural free functions accept `implements` / `representable` /
//! `assignable_to` as closures that receive the arenas as parameters (so the
//! same `TypeArena` the routine borrows `&mut` can be threaded into the
//! closure, rather than captured — which the borrow checker forbids). These
//! wrappers build those closures from the genuine implementations and hand
//! `self`'s arenas to the routines. The free functions are untouched and still
//! usable directly (e.g. with stub closures in tests).

use crate::hash::HashSet;

use guff::ast::Expr;
use guff_constant::{make_string, uint64_val};
use guff_types_errors::Code;

use crate::arena::{ObjectArena, ObjectData, PackageArena, TypeArena, TypeData, TypeId};
use crate::assignments::{assignable_to as assignable_to_fn, AssignableResult};
use crate::basic::BasicKind;
use crate::check::Checker;
use crate::check_expr_const::representable_const;
use crate::check_lookup::implements as implements_fn;
use crate::conversions::convertible_to as convertible_to_fn;
use crate::object::var::{new_var, VarKind};
use crate::operand::{Operand, OperandMode};
use crate::predicates::{
    default_type, has_nil, is_const_type, is_integer, is_non_type_param_interface, is_string,
    is_type_param, is_untyped, is_valid,
};
use crate::scope::lookup as scope_lookup;
use crate::stmt::unparen;
use crate::ObjectId;

/// The `implements` closure: does `v` implement interface `t` (non-constraint)?
fn implements_closure(
    a: &mut TypeArena,
    o: &ObjectArena,
    p: &PackageArena,
    v: TypeId,
    t: TypeId,
) -> bool {
    implements_fn(a, o, p, v, t, false).is_ok()
}

/// The `representable` closure: is operand `x` representable as `t`?
fn representable_closure(a: &TypeArena, x: &Operand, t: TypeId) -> bool {
    if let Some(v) = &x.val {
        return representable_const(a, v, t).is_some();
    }
    if x.is_nil() {
        return has_nil(a, t);
    }
    if let Some(xtyp) = x.typ {
        if is_untyped(a, xtyp) {
            if matches!(
                a.get(xtyp.underlying(a)),
                TypeData::Basic(b) if b.kind() == BasicKind::UntypedNil
            ) {
                return has_nil(a, t);
            }
        }
    }
    false
}

impl Checker {
    /// Is operand `x` assignable to a variable of type `target`?
    ///
    /// Checker-driven wrapper over [`crate::assignments::assignable_to`] with
    /// the real interface-satisfaction and constant-representability logic.
    pub fn assignable_to(&mut self, x: &Operand, target: TypeId) -> AssignableResult {
        // If either side is a generic instance, expand its methods first so the
        // interface-satisfaction check compares instantiated method signatures
        // (Go's lazy `Named.Method(i)`). No-op for non-instances.
        if let Some(v) = x.typ {
            self.expand_instance_methods(v);
            // Free `implements` cannot call `obj_decl`; resolve method sigs here
            // so `var _ I = (*T)(nil)` before method decls still typechecks.
            self.ensure_method_sigs(v);
        }
        self.expand_instance_methods(target);
        assignable_to_fn(
            &mut self.types,
            &self.objects,
            &self.packages,
            x,
            target,
            &implements_closure,
            &representable_closure,
        )
    }

    /// Is the conversion `target(x)` valid?
    ///
    /// Checker-driven wrapper over [`crate::conversions::convertible_to`].
    pub fn convertible_to(&mut self, x: &Operand, target: TypeId) -> bool {
        let assignable =
            |a: &mut TypeArena, o: &ObjectArena, p: &PackageArena, x: &Operand, t: TypeId| {
                assignable_to_fn(a, o, p, x, t, &implements_closure, &representable_closure).ok
            };
        convertible_to_fn(
            &mut self.types,
            &self.objects,
            &self.packages,
            x,
            target,
            &assignable,
        )
    }

    /// Type-check the conversion `T(x)` in place (the result is left in `x`).
    ///
    /// Equivalent to `Checker.conversion` (`conversions.go`). Handles constant
    /// conversions (with representability rounding and integer→string codepoint
    /// folding), the non-constant structural case via [`Self::convertible_to`],
    /// and the untyped-argument final-type update (so e.g. `[]byte("foo")`
    /// records the constant `"foo"` as `string`, not `[]byte`).
    ///
    /// DEFERRED: the constant-to-type-parameter branch falls back to
    /// `convertible_to` (precise per-term overflow causes are not produced);
    /// the `allString` check for the integer→string final-type case looks only
    /// at the target's underlying type (type-parameter targets are
    /// approximated).
    pub fn conversion(&mut self, x: &mut Operand, t: TypeId) {
        let const_arg = x.mode == OperandMode::Constant;
        let x_typ = x.typ.unwrap_or_else(|| self.invalid_type());
        let t_const = is_const_type(&self.types, t);

        let mut ok = false;

        if const_arg && t_const {
            // constant conversion
            ok = self.const_convertible_to(x, t);
            // An integer-constant → integer-type conversion can only fail on
            // overflow; give a concise error. (go.dev/issue/63563)
            if !ok && is_integer(&self.types, x_typ) && is_integer(&self.types, t) {
                let vs = x.val.as_ref().map(|v| v.to_string()).unwrap_or_default();
                let ts = self.type_str(t);
                self.error(
                    x.pos() as u32,
                    Code::InvalidConversion,
                    format!("constant {} overflows {}", vs, ts),
                );
                x.mode = OperandMode::Invalid;
                return;
            }
        } else if const_arg && is_type_param(&self.types, t) {
            // x converts to T if it converts to each specific type in T's type
            // set. DEFERRED: precise per-term overflow causes — reuse the
            // structural `convertible_to`, which recurses over the terms.
            ok = self.convertible_to(x, t);
            x.mode = OperandMode::Value; // type parameters are not constants
        } else if self.convertible_to(x, t) {
            // non-constant conversion
            ok = true;
            x.mode = OperandMode::Value;
        }

        if !ok {
            let (xs, ts) = (self.operand_str(x), self.type_str(t));
            self.error(
                x.pos() as u32,
                Code::InvalidConversion,
                format!("cannot convert {} to type {}", xs, ts),
            );
            x.mode = OperandMode::Invalid;
            return;
        }

        // For untyped values the conversion provides the type (spec: "A
        // constant may be given a type explicitly by a conversion"). Update the
        // recorded type of the argument expression accordingly.
        if is_untyped(&self.types, x_typ) {
            let untyped_nil = self.basic(BasicKind::UntypedNil);
            let mut final_ = t;
            if x_typ == untyped_nil {
                // keep T (isTypes2 && untyped nil argument)
            } else if is_non_type_param_interface(&self.types, t) || (const_arg && !t_const) {
                // default type (e.g. []byte("foo") records "foo" as string).
                final_ = default_type(&self.types, &self.typ, x_typ);
            } else if x.mode == OperandMode::Constant
                && is_integer(&self.types, x_typ)
                && self.all_string(t)
            {
                final_ = x_typ; // integer→string keeps the argument type
            }
            if let Some(e) = x.expr {
                self.update_expr_type(e, final_, true);
            }
        }

        x.typ = Some(t);
    }

    /// Reports whether the constant operand `x` is convertible to `t`'s (basic)
    /// underlying type, rounding `x.val` into that type in place. Equivalent to
    /// the inner `constConvertibleTo` closure of `Checker.conversion`.
    fn const_convertible_to(&mut self, x: &mut Operand, t: TypeId) -> bool {
        let tu = t.underlying(&self.types);
        if !matches!(self.types.get(tu), TypeData::Basic(_)) {
            return false;
        }
        if let Some(v) = &x.val {
            if let Some(rounded) = representable_const(&self.types, v, tu) {
                x.val = Some(rounded);
                return true;
            }
        }
        let x_typ = x.typ.unwrap_or_else(|| self.invalid_type());
        if is_integer(&self.types, x_typ) && is_string(&self.types, tu) {
            // integer → string: the value is a Unicode code point.
            const REPLACEMENT_CHAR: u32 = 0xFFFD;
            const MAX_RUNE: u64 = 0x0010_FFFF;
            let mut codepoint = REPLACEMENT_CHAR;
            if let Some(v) = &x.val {
                let (i, exact) = uint64_val(v);
                if exact && i <= MAX_RUNE {
                    codepoint = i as u32;
                }
            }
            let s = char::from_u32(codepoint).unwrap_or('\u{FFFD}').to_string();
            x.val = Some(make_string(s));
            return true;
        }
        false
    }

    /// Reports whether the target type's underlying type is a string type.
    /// Simplified `allString` (type-parameter targets are not walked).
    fn all_string(&self, t: TypeId) -> bool {
        is_string(&self.types, t.underlying(&self.types))
    }

    /// Check that operand `x` is assignable to type `target`, converting an
    /// untyped `x` to its target (or default) type along the way.
    ///
    /// Equivalent to `Checker.assignment`. `target == None` means the blank
    /// identifier `_`. `singleValue`, generic-function rejection, and `Info`
    /// recording are deferred.
    pub fn assignment(&mut self, x: &mut Operand, target: Option<TypeId>, context: &str) {
        // DEFERRED: singleValue(x).
        match x.mode {
            OperandMode::Invalid => return,
            OperandMode::NilValue
            | OperandMode::Constant
            | OperandMode::Variable
            | OperandMode::MapIndex
            | OperandMode::Value
            | OperandMode::CommaOk
            | OperandMode::CommaErr => {}
            _ => {
                let ts = target.map(|t| self.type_str(t)).unwrap_or_default();
                self.error(
                    x.pos() as u32,
                    guff_types_errors::Code::IncompatibleAssign,
                    format!("cannot assign to {} in {}", ts, context),
                );
                x.mode = OperandMode::Invalid;
                return;
            }
        }

        let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
        if is_untyped(&self.types, xtyp) {
            // Determine the conversion target.
            let conv_target = match target {
                None => Some(self.default_for(xtyp, x, context)),
                Some(t) if is_non_type_param_interface_t(self, t) => {
                    if xtyp == self.basic(BasicKind::UntypedNil) || x.is_nil() {
                        // Untyped nil → interface is a typed nil of the target
                        // interface (not default_for, which rejects nil).
                        Some(t)
                    } else {
                        Some(self.default_for(xtyp, x, context))
                    }
                }
                Some(t) => Some(t),
            };
            let conv_target = match conv_target {
                Some(t) => t,
                None => {
                    x.mode = OperandMode::Invalid;
                    return;
                }
            };
            self.convert_untyped(x, conv_target);
            if x.mode == OperandMode::Invalid {
                return;
            }
        }

        // x.typ is now typed. If target is the blank identifier, we're done.
        let target = match target {
            Some(t) => t,
            None => return,
        };

        let res = self.assignable_to(x, target);
        if !res.ok {
            let (xs, ts) = (
                x.typ.map(|t| self.type_str(t)).unwrap_or_default(),
                self.type_str(target),
            );
            let code = res
                .code
                .unwrap_or(guff_types_errors::Code::IncompatibleAssign);
            self.error(
                x.pos() as u32,
                code,
                format!("cannot use {} value as {} value in {}", xs, ts, context),
            );
            x.mode = OperandMode::Invalid;
        }
    }

    /// The default type for an untyped operand being assigned without a target
    /// (or to an interface). Reports `UntypedNilUse` for untyped nil.
    fn default_for(&mut self, xtyp: TypeId, x: &mut Operand, context: &str) -> TypeId {
        if xtyp == self.basic(BasicKind::UntypedNil) || x.is_nil() {
            self.error(
                x.pos() as u32,
                guff_types_errors::Code::UntypedNilUse,
                format!("use of untyped nil in {}", context),
            );
            return self.invalid_type();
        }
        default_type(&self.types, &self.typ, xtyp)
    }

    /// Initialize a package-level (or local) constant from operand `x`.
    ///
    /// Equivalent to `Checker.initConst`.
    pub fn init_const(&mut self, lhs: ObjectId, x: &mut Operand) {
        let lhs_typ = lhs.typ(&self.objects);
        let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
        // The lhs always exists with a Typ[Invalid] placeholder (resolver), so
        // treat "invalid placeholder" as "no type yet".
        let lhs_has_type = lhs_typ.map(|t| is_valid(&self.types, t)).unwrap_or(false);

        if x.mode == OperandMode::Invalid || !is_valid(&self.types, xtyp) {
            self.set_const_typ(lhs, self.invalid_type());
            return;
        }
        if x.mode != OperandMode::Constant {
            self.error(
                x.pos() as u32,
                guff_types_errors::Code::InvalidConstInit,
                "initializer is not constant",
            );
            self.set_const_typ(lhs, self.invalid_type());
            return;
        }

        // If the lhs has no explicit type yet, adopt x's type.
        let target = if lhs_has_type {
            lhs_typ.unwrap()
        } else {
            self.set_const_typ(lhs, xtyp);
            xtyp
        };

        self.assignment(x, Some(target), "constant declaration");
        if x.mode == OperandMode::Invalid {
            return;
        }
        if let Some(v) = x.val.clone() {
            if let ObjectData::Const(c) = self.objects.get_mut(lhs) {
                c.set_val(v);
            }
        }
    }

    /// Initialize a variable from operand `x`.
    ///
    /// Equivalent to `Checker.initVar`.
    pub fn init_var(&mut self, lhs: ObjectId, x: &mut Operand, context: &str) {
        let lhs_typ = lhs.typ(&self.objects);
        let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
        let lhs_has_type = lhs_typ.map(|t| is_valid(&self.types, t)).unwrap_or(false);

        // Match Go: only assign Typ[Invalid] when lhs has no type yet (nil in
        // go/types). If varDecl already set an explicit type (e.g. time.UTC's
        // `*Location`), keep it — a forward-ref init like `&utcLoc` must not
        // clobber the declared type (go/types assignments.go initVar).
        if x.mode == OperandMode::Invalid || !is_valid(&self.types, xtyp) {
            if !lhs_has_type {
                self.set_var_typ(lhs, self.invalid_type());
            }
            x.mode = OperandMode::Invalid;
            return;
        }

        let target = if lhs_has_type {
            lhs_typ.unwrap()
        } else {
            // Adopt x's type, converting untyped to its default.
            let mut t = xtyp;
            if is_untyped(&self.types, t) {
                if t == self.basic(BasicKind::UntypedNil) {
                    self.error(
                        x.pos() as u32,
                        guff_types_errors::Code::UntypedNilUse,
                        format!("use of untyped nil in {}", context),
                    );
                    self.set_var_typ(lhs, self.invalid_type());
                    x.mode = OperandMode::Invalid;
                    return;
                }
                t = default_type(&self.types, &self.typ, t);
            }
            self.set_var_typ(lhs, t);
            t
        };

        self.assignment(x, Some(target), context);
    }

    /// Determine the type of an assignment's left-hand side and check that it
    /// is assignable to. Returns `None` for the blank identifier `_`,
    /// `Some(Typ[Invalid])` on error, and `Some(t)` for a valid target.
    ///
    /// Equivalent to `Checker.lhsVar` (the `usedVars` save/restore is omitted —
    /// use tracking is deferred).
    fn lhs_var(&mut self, lhs: &Expr) -> Option<TypeId> {
        // The blank identifier is not evaluated.
        if let Expr::Ident(id) = unparen(lhs) {
            if id.name == "_" {
                return None;
            }
        }

        let mut x = Operand::invalid();
        self.expr(&mut x, lhs);

        let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
        if x.mode == OperandMode::Invalid || !is_valid(&self.types, xtyp) {
            return Some(self.invalid_type());
        }

        // spec: "Each left-hand side operand must be addressable, a map index
        // expression, or the blank identifier."
        match x.mode {
            OperandMode::Variable | OperandMode::MapIndex => Some(xtyp),
            _ => {
                let xs = self.operand_str(&x);
                self.error(
                    x.pos() as u32,
                    Code::UnassignableOperand,
                    format!(
                        "cannot assign to {} (neither addressable nor a map index expression)",
                        xs
                    ),
                );
                Some(self.invalid_type())
            }
        }
    }

    /// Check the assignment `lhs = rhs` (when `x` is `None`), or `lhs = x` (when
    /// `x` is the already-evaluated rhs operand; `rhs` is then ignored).
    ///
    /// Equivalent to `Checker.assignVar`.
    pub fn assign_var(
        &mut self,
        lhs: &Expr,
        rhs: Option<&Expr>,
        x: Option<Operand>,
        context: &str,
    ) {
        let t = self.lhs_var(lhs); // None = blank `_`
        if let Some(tid) = t {
            if !is_valid(&self.types, tid) {
                // lhs is invalid; still evaluate rhs for its errors.
                if x.is_none() {
                    if let Some(r) = rhs {
                        let mut tmp = Operand::invalid();
                        self.expr(&mut tmp, r);
                    }
                }
                return;
            }
        }

        let mut xop = match x {
            Some(x) => x,
            None => {
                let mut xx = Operand::invalid();
                if let Some(r) = rhs {
                    self.expr(&mut xx, r);
                }
                xx
            }
        };

        let ctx = if t.is_none() && context == "assignment" {
            "assignment to _ identifier"
        } else {
            context
        };
        self.assignment(&mut xop, t, ctx);
    }

    /// Type-check assignments of `rhs` expressions to `lhs` expressions.
    ///
    /// Equivalent to `Checker.assignVars`. The n:1 multi-valued spread
    /// (`multiExpr`, needs call-result expansion) is DEFERRED (30c).
    pub fn assign_vars(&mut self, lhs: &[Expr], rhs: &[Expr]) {
        let (l, r) = (lhs.len(), rhs.len());

        // n:1 — a single, possibly multi-valued, right-hand side
        // (e.g. `a, b = f()`).
        if r == 1 && l != 1 {
            let (mut values, comma_ok) = self.eval_multi(&rhs[0], l);
            if values.len() == l {
                // Capture the value types before the operands are consumed.
                let t0 = values[0].typ.unwrap_or_else(|| self.invalid_type());
                let t1 = values
                    .get(1)
                    .and_then(|o| o.typ)
                    .unwrap_or_else(|| self.invalid_type());
                let err_before = self.errors.len();
                for (i, x) in values.iter_mut().enumerate() {
                    self.assign_var(&lhs[i], None, Some(std::mem::take(x)), "assignment");
                }
                // Only record the comma-ok 2-tuple if both assignments
                // succeeded (go.dev/issue/59371) — proxied by "no new error".
                if comma_ok && self.errors.len() == err_before {
                    self.record_comma_ok_types(&rhs[0], t0, t1);
                }
                return;
            }
            self.assign_error(rhs, l, values.len());
            for e in lhs {
                let mut tmp = Operand::invalid();
                self.expr(&mut tmp, e);
            }
            return;
        }

        if l == r {
            for i in 0..l {
                self.assign_var(&lhs[i], Some(&rhs[i]), None, "assignment");
            }
            return;
        }

        // l != r and r != 1: a genuine count mismatch.
        self.assign_error(rhs, l, r);
        for e in lhs {
            let mut tmp = Operand::invalid();
            self.expr(&mut tmp, e);
        }
        for e in rhs {
            let mut tmp = Operand::invalid();
            self.expr(&mut tmp, e);
        }
    }

    /// Type-check assignments of initialization expressions `rhs` to the
    /// (already-created) variables `lhs`. If `is_return`, this is the implicit
    /// assignment of result expressions to result parameters.
    ///
    /// Equivalent to `Checker.initVars`. The n:1 multi-valued spread is
    /// DEFERRED (30c).
    pub fn init_vars(&mut self, lhs: &[ObjectId], rhs: &[Expr], is_return: bool) {
        let context = if is_return {
            "return statement"
        } else {
            "assignment"
        };
        let (l, r) = (lhs.len(), rhs.len());

        // n:1 — a single, possibly multi-valued, right-hand side (e.g. a call
        // returning a tuple: `a, b := f()`).
        if r == 1 && l != 1 {
            let (mut values, comma_ok) = self.eval_multi(&rhs[0], l);
            if values.len() == l {
                let err_before = self.errors.len();
                for (i, x) in values.iter_mut().enumerate() {
                    self.init_var(lhs[i], x, context);
                }
                // Only record the comma-ok 2-tuple if both initializations
                // succeeded (go.dev/issue/59371) — proxied by "no new error".
                if comma_ok && self.errors.len() == err_before {
                    let t0 = values[0].typ.unwrap_or_else(|| self.invalid_type());
                    let t1 = values[1].typ.unwrap_or_else(|| self.invalid_type());
                    self.record_comma_ok_types(&rhs[0], t0, t1);
                }
                return;
            }
            let got = values.len();
            if is_return {
                self.return_error(rhs, l, got);
            } else {
                self.assign_error(rhs, l, got);
            }
            // Vars keep their `Typ[Invalid]` placeholder.
            return;
        }

        if l == r {
            for i in 0..l {
                let mut x = Operand::invalid();
                self.expr(&mut x, &rhs[i]);
                self.init_var(lhs[i], &mut x, context);
            }
            return;
        }

        // l != r and r != 1: a genuine count mismatch.
        if is_return {
            self.return_error(rhs, l, r);
        } else {
            self.assign_error(rhs, l, r);
        }
        for e in rhs {
            let mut tmp = Operand::invalid();
            self.expr(&mut tmp, e);
        }
    }

    /// Evaluate a single expression that may yield multiple values, returning
    /// one operand per value. A call (or other expression) whose type is a
    /// tuple is unpacked into one `Value` operand per tuple element; any other
    /// expression yields a single operand.
    ///
    /// Equivalent to `Checker.multiExpr`: returns the per-value operands plus a
    /// `comma_ok` flag. When `want == 2`, a comma-ok-able operand (a map index,
    /// type assertion, or channel receive) is expanded to `(value, bool)` and
    /// the flag is `true`; a tuple-valued expression (e.g. a multi-return call)
    /// is unpacked with the flag `false`.
    fn eval_multi<'e>(&mut self, e: &'e Expr, want: usize) -> (Vec<Operand<'e>>, bool) {
        let mut x = Operand::invalid();
        self.expr(&mut x, e);
        if x.mode == OperandMode::Invalid {
            return (vec![x], false);
        }
        if let Some(t) = x.typ {
            if matches!(self.types.get(t), TypeData::Tuple(_)) {
                let n = crate::tuple::tuple_len(&self.types, Some(t));
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let v = crate::tuple::tuple_at(&self.types, t, i);
                    let mut op = Operand::invalid();
                    op.mode = OperandMode::Value;
                    op.typ = v.typ(&self.objects);
                    op.expr = x.expr;
                    out.push(op);
                }
                return (out, false);
            }
        }
        // Comma-ok form: `v, ok := m[k]` / `v, ok := x.(T)` / `v, ok := <-ch`.
        // The first value is the operand's type; the second is a plain `bool`.
        if want == 2 && matches!(x.mode, OperandMode::MapIndex | OperandMode::CommaOk) {
            let value_typ = x.typ;
            let mut value = Operand::invalid();
            value.mode = OperandMode::Value;
            value.typ = value_typ;
            value.expr = x.expr;
            let mut ok = Operand::invalid();
            ok.mode = OperandMode::Value;
            ok.typ = Some(self.basic(BasicKind::Bool));
            ok.expr = x.expr;
            return (vec![value, ok], true);
        }
        (vec![x], false)
    }

    /// Type-check a short variable declaration `lhs := rhs`.
    ///
    /// Equivalent to `Checker.shortVarDecl`.
    pub fn short_var_decl(&mut self, lhs: &[Expr], rhs: &[Expr]) {
        let top = self.delayed.len();
        let scope = match self.env.scope {
            Some(s) => s,
            None => return,
        };

        let mut seen: HashSet<String> = HashSet::default();
        let mut lhs_vars: Vec<ObjectId> = Vec::with_capacity(lhs.len());
        let mut new_vars: Vec<ObjectId> = Vec::new();
        let mut has_err = false;

        for l in lhs {
            let ident = match l {
                Expr::Ident(id) => Some(id),
                _ => None,
            };
            let id = match ident {
                Some(id) => id,
                None => {
                    self.error(
                        l.pos().0 as u32,
                        Code::BadDecl,
                        "non-name on left side of :=",
                    );
                    has_err = true;
                    let invalid = self.invalid_type();
                    lhs_vars.push(new_var(&mut self.objects, "_", invalid));
                    continue;
                }
            };

            let name = id.name.clone();
            if name != "_" {
                if seen.contains(&name) {
                    self.error(
                        l.pos().0 as u32,
                        Code::RepeatedDecl,
                        format!("{} repeated on left side of :=", name),
                    );
                    has_err = true;
                    let invalid = self.invalid_type();
                    lhs_vars.push(new_var(&mut self.objects, "_", invalid));
                    continue;
                }
                seen.insert(name.clone());
            }

            // Redeclaration: the variable's scope starts after the declaration,
            // so use a same-scope lookup and declare (insert) later.
            if let Some(alt) = scope_lookup(&self.scopes, scope, &name) {
                self.record_use(id, alt);
                if matches!(self.objects.get(alt), ObjectData::Var(_)) {
                    lhs_vars.push(alt);
                } else {
                    self.error(
                        l.pos().0 as u32,
                        Code::UnassignableOperand,
                        format!("cannot assign to {}", name),
                    );
                    has_err = true;
                    let invalid = self.invalid_type();
                    lhs_vars.push(new_var(&mut self.objects, "_", invalid));
                }
                continue;
            }

            // Declare a new variable.
            let invalid = self.invalid_type();
            let obj = new_var(&mut self.objects, name.clone(), invalid);
            obj.set_pkg(&mut self.objects, self.pkg);
            obj.set_pos(&mut self.objects, id.pos().0 as u32);
            if let ObjectData::Var(v) = self.objects.get_mut(obj) {
                v.set_kind(VarKind::Local);
            }
            lhs_vars.push(obj);
            if name != "_" {
                new_vars.push(obj);
            }
            // Record the definition eagerly (Go's shortVarDecl records the def
            // here, then declares with a nil id below). Recorded for `_` too,
            // matching Go's `recordDef`.
            self.record_def(id, Some(obj));
        }

        self.init_vars(&lhs_vars, rhs, false);

        // Process function literals in rhs expressions before scope changes.
        self.process_delayed(top);

        if new_vars.is_empty() && !has_err {
            let pos = lhs.first().map(|e| e.pos().0 as u32).unwrap_or(0);
            self.error(pos, Code::NoNewVar, "no new variables on left side of :=");
            return;
        }

        // spec: the scope of a short-var-declared identifier begins at the end
        // of the ShortVarDecl.
        let scope_pos = rhs.last().map(|e| e.end().0 as u32).unwrap_or(0);
        for obj in new_vars {
            self.declare(scope, obj, scope_pos);
        }
    }

    /// Report an assignment-count mismatch (`l` variables but `r` values).
    ///
    /// Simplified `Checker.assignError` (no special-casing of a single call
    /// expression's result count).
    fn assign_error(&mut self, rhs: &[Expr], l: usize, r: usize) {
        let pos = rhs.first().map(|e| e.pos().0 as u32).unwrap_or(0);
        let vars = if l == 1 { "variable" } else { "variables" };
        let vals = if r == 1 { "value" } else { "values" };
        self.error(
            pos,
            Code::WrongAssignCount,
            format!("assignment mismatch: {} {} but {} {}", l, vars, r, vals),
        );
    }

    /// Report a return-value-count mismatch (`l` results expected, `r` given).
    ///
    /// Simplified `Checker.returnError`: reports the "not enough" / "too many
    /// return values" qualifier with a `WrongResultCount` code. The
    /// have/want type-summary secondary lines are dropped.
    fn return_error(&mut self, rhs: &[Expr], l: usize, r: usize) {
        // Report at the first extra value (too many) or the last value (too
        // few, when there is one); otherwise at the first rhs / statement.
        let qualifier = if r > l { "too many" } else { "not enough" };
        let at = if r > l {
            rhs.get(l).map(|e| e.pos().0 as u32)
        } else if r > 0 {
            rhs.get(r - 1).map(|e| e.pos().0 as u32)
        } else {
            None
        };
        let pos = at.or_else(|| rhs.first().map(|e| e.pos().0 as u32)).unwrap_or(0);
        self.error(
            pos,
            Code::WrongResultCount,
            format!("{} return values", qualifier),
        );
    }

    fn set_const_typ(&mut self, obj: ObjectId, typ: TypeId) {
        if let ObjectData::Const(c) = self.objects.get_mut(obj) {
            c.set_typ(typ);
        }
    }

    pub(crate) fn set_var_typ(&mut self, obj: ObjectId, typ: TypeId) {
        if let ObjectData::Var(v) = self.objects.get_mut(obj) {
            v.set_typ(typ);
        }
    }
}

/// Helper: is `t` a non-type-parameter interface? (Kept free to avoid borrow
/// gymnastics inside the `assignment` match.)
fn is_non_type_param_interface_t(check: &Checker, t: TypeId) -> bool {
    crate::predicates::is_non_type_param_interface(&check.types, t)
}
