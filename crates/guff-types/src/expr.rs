//! Port of expression type-checking from `go/types/expr.go` (+ `ident` from
//! `typexpr.go`).
//!
//! **Chunk 25a**: the dispatch skeleton ([`Checker::expr`] / [`Checker::raw_expr`]
//! / [`Checker::expr_internal`]) and the identifier case ([`Checker::ident`]).
//! **Chunk 25b**: basic literals ([`Checker::basic_lit`]) and unary operators
//! ([`Checker::unary`]: `+`/`-`/`^`/`!` with constant folding, plus `&`).
//! **Chunk 25c**: binary operators ([`Checker::binary`]) with `match_types`,
//! comparisons ([`Checker::comparison`]) and shifts ([`Checker::shift`]), all
//! with constant folding. Remaining kinds stay `// DEFERRED`.
//!
//! `Checker.expr` takes an out-`Operand` it fills in (mode/typ/val), matching
//! Go's `check.expr(T, x, e)` minus the assignment target `T` and the generic
//! hint plumbing (deferred).
//!
//! ## Deferrals (chunk-25a, see §8)
//!
//! - target `T`/`hint`, `genericExpr`/`exprOrType`/`exprWithHint`,
//!   `nonGeneric`/`pendingType`, and `record`/`recordUse` are omitted (no Info
//!   recording — §18b). `singleValue` landed 2026-08-11; `exclude` has not, so
//!   `x := none()` reports "cannot assign to func()" where Go says
//!   "none() (no value) used as value".
//! - `usedVars`/`addDeclDep` (use & dependency tracking), dot-import marking,
//!   `verifyVersionf` gates on `any`/`comparable`, and the broken-alias check
//!   are omitted.
//! - all non-`Ident` expression kinds are DEFERRED (set the operand invalid):
//!   BasicLit + unary (25b), binary/compare/shift (25c), convertUntyped (25d),
//!   CompositeLit/FuncLit (literals, 25e), Selector (26), Index/Slice (28),
//!   Call (27), and the type-expression forms.

use guff::ast::{BasicLit, BinaryExpr, Expr, Ident, StarExpr, UnaryExpr};
use guff::token::Token;
use guff_constant::{binary_op, compare, make_bool, shift, sign, uint64_val, unary_op, Value};
use guff_types_errors::Code;

use crate::arena::{ObjectData, TypeData, TypeId};
use crate::basic::BasicKind;
use crate::check::Checker;
use crate::object::builtin::ExprKind;
use crate::operand::{Operand, OperandMode};
use crate::conversions::is_pointer;
use crate::pointer::{new_pointer, pointer_elem};
use crate::predicates::{
    all_boolean, all_integer, all_numeric, all_numeric_or_string, all_ordered, comparable,
    default_type, has_nil, identical, is_integer, is_interface, is_type_param, is_typed,
    is_unsigned, is_untyped, is_valid,
};

impl Checker {
    /// Type-check the expression `e`, recording the result in `x`.
    ///
    /// Equivalent to `Checker.expr` (without the assignment target).
    pub fn expr<'a>(&mut self, x: &mut Operand<'a>, e: &'a Expr) -> ExprKind {
        let kind = self.raw_expr(x, e, None);
        // DEFERRED: exclude(x, novalue|builtin|typexpr).
        self.single_value(x);
        kind
    }

    /// Type-check `e` where `hint` is the element type of the enclosing
    /// composite literal (so a typeless inner `{...}` picks it up).
    ///
    /// Equivalent to `Checker.exprWithHint`. Only a bare `CompositeLit`
    /// consumes the hint; every other expression form ignores it.
    pub fn expr_with_hint<'a>(
        &mut self,
        x: &mut Operand<'a>,
        e: &'a Expr,
        hint: TypeId,
    ) -> ExprKind {
        let kind = self.raw_expr(x, e, Some(hint));
        self.single_value(x);
        kind
    }

    /// Reduce a multi-valued operand to a single value, reporting
    /// `multiple-value f() in single-value context` when it cannot be.
    ///
    /// Equivalent to `Checker.singleValue`. Every context that legitimately
    /// consumes a tuple (`a, b := f()`, `g(f())`, `return f()`) goes through
    /// [`Checker::raw_expr`] instead, exactly as Go's `multiExpr` / `exprList`
    /// do — so reaching here with a tuple *is* the error.
    fn single_value(&mut self, x: &mut Operand<'_>) {
        if x.mode != OperandMode::Value {
            return;
        }
        // Tuple types are never named, so there is no need to go through the
        // underlying type here.
        let Some(t) = x.typ else { return };
        if !matches!(self.types.get(t), TypeData::Tuple(_)) {
            return;
        }
        let what = match x.expr {
            Some(e) => format!(
                "{} (value of type {})",
                crate::exprstring::expr_string(e),
                self.type_str(t)
            ),
            None => format!("value of type {}", self.type_str(t)),
        };
        self.error(
            x.pos() as u32,
            Code::WrongResultCount,
            format!("multiple-value {} in single-value context", what),
        );
        x.mode = OperandMode::Invalid;
    }

    /// Type-check `e` as a type expression when possible, otherwise as a value.
    ///
    /// Equivalent to `Checker.exprOrType` (without the `allowGeneric` path).
    pub(crate) fn expr_or_type<'a>(&mut self, x: &mut Operand<'a>, e: &'a Expr) {
        // Probing as a type is not free: `typ` *reports* on the way, so
        // `(*p)()` — a call through a `*func()` variable, which is syntactically
        // a pointer conversion — left "p is not a type" behind even though the
        // value path below then handled it, and the whole package went
        // ill-typed for it. Nothing in a finding-set diff shows that
        // (`compat/health.py`); rclone's `lib/atexit` and `fs/rc/jobs` are two
        // packages of exactly this shape. Roll the probe's diagnostics back,
        // the way `builtin_new` already does for `new(x)`.
        let mark = self.errors.len();
        let t = self.typ(e);
        if is_valid(&self.types, t) {
            x.mode = OperandMode::TypeExpr;
            x.typ = Some(t);
            x.expr = Some(e);
            self.record_type_and_value(e, OperandMode::TypeExpr, t, None);
        } else {
            self.errors.truncate(mark);
            self.expr(x, e);
        }
    }

    /// The dispatch wrapper around [`Checker::expr_internal`].
    ///
    /// Equivalent to `Checker.rawExpr` (minus `nonGeneric`/`pendingType`/
    /// `record`, which are deferred). `hint`, when set, is the composite
    /// literal element type threaded to a bare inner `{...}`. Returns the
    /// expression's [`ExprKind`] (conversion/expression/statement).
    pub(crate) fn raw_expr<'a>(
        &mut self,
        x: &mut Operand<'a>,
        e: &'a Expr,
        hint: Option<TypeId>,
    ) -> ExprKind {
        let kind = self.expr_internal(x, e, hint);
        // Go's `rawExpr` tail sets `x.expr = e` before recording, so the
        // delayed-untyped machinery (`update_expr_type`, which keys on the
        // operand's expr) sees the full source node. Mirror that here.
        x.expr = Some(e);
        // Record the type (and constant value) of `e` in `Info.Types`
        // (Go's `rawExpr` tail `check.record(x)`, chunk 50).
        self.record(x, e);
        // DEFERRED: nonGeneric(x), pendingType(x).
        kind
    }

    /// The core expression dispatch.
    ///
    /// Equivalent to `Checker.exprInternal` (chunk-25a subset).
    fn expr_internal<'a>(
        &mut self,
        x: &mut Operand<'a>,
        e: &'a Expr,
        hint: Option<TypeId>,
    ) -> ExprKind {
        // Ensure a valid invalid-state on bailout (Go's go.dev/issue/5770).
        x.mode = OperandMode::Invalid;
        x.typ = Some(self.invalid_type());

        // All expression forms yield `Expression` except calls (which yield the
        // call's kind — conversion/expression/statement) and parentheses (which
        // pass the inner kind straight through). Go's `exprInternal` ends with
        // `return expression` for everything it doesn't hand off to `callExpr`.
        match e {
            Expr::BadExpr(_) => { /* error reported before — leave invalid */ }
            Expr::Ident(id) => self.ident(x, id, false),
            Expr::BasicLit(lit) => self.basic_lit(x, lit),
            // Type inference doesn't go past parentheses (go.dev/issue/29316),
            // so the composite-literal `hint` is dropped here. The inner kind
            // propagates, so `(f())` is still a valid statement.
            //
            // This recurses through `raw_expr`, not `expr_internal`, exactly as
            // Go does — the inner node needs its own `Info.Types` entry. Going
            // straight to `expr_internal` records only the `ParenExpr`, and a
            // consumer that looks the inner node up by itself finds nothing:
            // `(func(input bool) *bool { return &input })(false)` left the SSA
            // builder with a signature-less function literal, whose parameters
            // then could not be declared, so `input` looked like a captured
            // free variable.
            Expr::ParenExpr(p) => return self.raw_expr(x, &p.x, None),
            Expr::StarExpr(st) => self.star_expr(x, st),
            // Channel receive `<-ch` is a valid statement (select case / bare
            // ExprStmt). Go's exprInternal returns `statement` for `token.ARROW`.
            Expr::UnaryExpr(u) => {
                self.unary(x, u);
                if u.op == Token::ARROW {
                    return ExprKind::Statement;
                }
            }
            Expr::BinaryExpr(b) => self.binary(x, b),
            Expr::SelectorExpr(_) => self.selector(x, e, false),
            Expr::CallExpr(_) => return self.call_expr(x, e),
            Expr::IndexExpr(ie) => {
                // A generic function value `f[targs]` used as a value: `index_expr`
                // signals it, then `func_inst` instantiates the signature.
                if self.index_expr(x, ie) {
                    // Value position: there is no argument list to learn the
                    // remaining type arguments from, so `infer` is true and a
                    // partial instantiation is an error here (Go: `funcInst`
                    // with a nil target).
                    self.func_inst(
                        x,
                        &ie.x,
                        std::slice::from_ref(&*ie.index),
                        ie.x.pos().0 as u32,
                        true,
                    );
                }
            }
            Expr::IndexListExpr(ie) => {
                // Multi-argument explicit instantiation `f[T1, T2]` as a value.
                self.expr(x, &ie.x);
                if self.is_generic_func_value(x) {
                    self.func_inst(x, &ie.x, &ie.indices, ie.x.pos().0 as u32, true);
                } else if x.mode != OperandMode::Invalid {
                    let xs = self.operand_str(x);
                    self.error(
                        ie.x.pos().0 as u32,
                        guff_types_errors::Code::NonSliceableOperand,
                        format!("cannot index {}", xs),
                    );
                    x.mode = OperandMode::Invalid;
                    x.typ = Some(self.invalid_type());
                }
            }
            Expr::SliceExpr(se) => self.slice_expr(x, se),
            Expr::FuncLit(fl) => self.func_lit(x, fl),
            Expr::CompositeLit(cl) => self.composite_lit(x, cl, hint),
            Expr::TypeAssertExpr(ta) => self.type_assert(x, ta),

            // DEFERRED (later sub-chunks): every other expression form leaves
            // the operand invalid for now.
            //   type-expression forms → later
            _ => {}
        }
        ExprKind::Expression
    }

    /// Type-check a pointer dereference `*x` — or the pointer *type* `*T`.
    ///
    /// When the operand denotes a type, `*T` is the pointer type — that is how
    /// a method expression on a pointer receiver (`(*T).Foo`) is spelled.
    /// Without this branch the type case fell through to the indirection path
    /// and was rejected outright with "invalid indirect of T (Type)".
    fn star_expr<'a>(&mut self, x: &mut Operand<'a>, e: &'a StarExpr) {
        self.expr(x, &e.x);
        if x.mode == OperandMode::Invalid {
            return;
        }
        if x.mode == OperandMode::TypeExpr {
            let elem = x.typ.unwrap_or_else(|| self.invalid_type());
            x.typ = Some(crate::pointer::new_pointer(&mut self.types, elem));
            return;
        }
        let typ = x.typ.unwrap_or_else(|| self.invalid_type());
        if !is_pointer(&self.types, typ) {
            let xs = self.operand_str(x);
            self.error(
                e.star.0 as u32,
                Code::InvalidIndirection,
                format!("invalid indirect of {}", xs),
            );
            x.mode = OperandMode::Invalid;
            return;
        }
        // A pointer indirection is addressable **whatever the pointer
        // expression was** — the spec lists it alongside "a variable" and "a
        // slice indexing operation", and go/types sets `x.mode = variable`
        // unconditionally here. Requiring the operand itself to be addressable
        // rejected `*(*Sample)(ptr) = …` (deref of a conversion) and
        // `*getPtrFunc(app) = …` (deref of a call result), which are five
        // ill-typed packages across thanos, argo-cd and cli.
        x.mode = OperandMode::Variable;
        x.typ = Some(pointer_elem(&self.types, typ));
    }

    /// Type-check a basic literal (int/float/imag/char/string).
    ///
    /// Equivalent to `Checker.basicLit` — the overflow recheck (`overflow`)
    /// and the go1.13 digit-separator `langCompat` gate are deferred.
    fn basic_lit<'a>(&mut self, x: &mut Operand<'a>, e: &BasicLit) {
        let kind = match e.kind {
            Some(k) => k,
            None => {
                x.mode = OperandMode::Invalid;
                return;
            }
        };
        // Cap absurdly long constants (go/constant would choke).
        if matches!(kind, Token::INT | Token::FLOAT | Token::IMAG) && e.value.len() > 10000 {
            self.error(
                e.value_pos.0 as u32,
                Code::InvalidConstVal,
                "excessively long constant",
            );
            x.mode = OperandMode::Invalid;
            return;
        }
        x.set_const(&self.typ, kind, &e.value);
        if x.mode == OperandMode::Invalid {
            self.error(
                e.value_pos.0 as u32,
                Code::InvalidConstVal,
                format!("malformed constant: {}", e.value),
            );
            return;
        }
        // expr set by raw_expr
        self.overflow(x, e.value_pos.0 as u32, "");
    }

    /// Type-check a type assertion `x.(T)`.
    ///
    /// Equivalent to the `*syntax.AssertExpr` case in `Checker.exprInternal`.
    /// The `.(type)` form (`ty == None`) is only legal inside a type switch
    /// guard, which is handled directly in `stmt.rs`; reaching it here is a
    /// syntax error.
    fn type_assert<'a>(&mut self, x: &mut Operand<'a>, e: &'a guff::ast::TypeAssertExpr) {
        self.expr(x, &e.x);
        if x.mode == OperandMode::Invalid {
            return;
        }
        // x.(type) expressions are encoded with a nil type and only valid in a
        // type switch.
        let ty = match &e.ty {
            Some(t) => t,
            None => {
                self.error(
                    e.lparen.0 as u32,
                    Code::InvalidSyntaxTree,
                    "use of .(type) outside type switch",
                );
                x.mode = OperandMode::Invalid;
                return;
            }
        };

        let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
        if is_type_param(&self.types, xtyp) {
            let xs = self.operand_str(x);
            self.error(
                e.x.pos().0 as u32,
                Code::InvalidAssert,
                format!(
                    "invalid operation: cannot use type assertion on type parameter value {}",
                    xs
                ),
            );
            x.mode = OperandMode::Invalid;
            return;
        }
        let xu = xtyp.underlying(&self.types);
        if !is_interface(&self.types, xu) {
            let xs = self.operand_str(x);
            self.error(
                e.x.pos().0 as u32,
                Code::InvalidAssert,
                format!("invalid operation: {} is not an interface", xs),
            );
            x.mode = OperandMode::Invalid;
            return;
        }

        let t = self.typ(ty);
        if !is_valid(&self.types, t) {
            x.mode = OperandMode::Invalid;
            return;
        }
        self.type_assertion(e.lparen.0 as u32, x, t, false);
        x.mode = OperandMode::CommaOk;
        x.typ = Some(t);
        // expr set by raw_expr
    }

    /// Type-check a unary expression.
    ///
    /// Equivalent to `Checker.unary` for the value/constant operators
    /// (`+`, `-`, `^`, `!`) and address-of (`&`). The channel receive (`<-`,
    /// needs `chanElem`) and `~` (type-constraint only) cases are DEFERRED.
    fn unary<'a>(&mut self, x: &mut Operand<'a>, e: &'a UnaryExpr) {
        self.expr(x, &e.x);
        if x.mode == OperandMode::Invalid {
            return;
        }
        let op = e.op;
        let typ = x.typ.unwrap_or_else(|| self.invalid_type());

        match op {
            // Address-of: &x yields *typeof(x).
            Token::AND => {
                // spec: "As an exception to the addressability requirement, x
                // may also be a (possibly parenthesized) composite literal."
                // (Go: `check.unary`, `token.AND`.)
                let mut operand = e.x.as_ref();
                while let Expr::ParenExpr(p) = operand {
                    operand = p.x.as_ref();
                }
                let is_composite_lit = matches!(operand, Expr::CompositeLit(_));
                let is_star_deref = matches!(operand, Expr::StarExpr(_));
                if !is_composite_lit && !is_star_deref && x.mode != OperandMode::Variable {
                    self.error(
                        e.x.pos().0 as u32,
                        Code::UnaddressableOperand,
                        "cannot take address of operand",
                    );
                    x.mode = OperandMode::Invalid;
                    return;
                }
                x.mode = OperandMode::Value;
                x.typ = Some(new_pointer(&mut self.types, typ));
                return;
            }
            // Channel receive: `<-ch` yields the channel's element type.
            Token::ARROW => {
                // `Checker.chanElem` reaches the channel through `commonUnder`,
                // so `<-c` works when `c`'s type parameter has a single common
                // channel underlying type.
                let (common, _err) = crate::under::common_under(
                    &mut self.types,
                    &self.objects,
                    &self.packages,
                    typ,
                    None,
                );
                let u = common.unwrap_or_else(|| typ.underlying(&self.types));
                let chan = match self.types.get(u) {
                    crate::arena::TypeData::Chan(_) => u,
                    _ => {
                        let xs = self.operand_str(x);
                        self.error(
                            e.op_pos.0 as u32,
                            Code::InvalidReceive,
                            format!("cannot receive from non-channel {}", xs),
                        );
                        x.mode = OperandMode::Invalid;
                        return;
                    }
                };
                if crate::chan::chan_dir(&self.types, chan) == crate::chan::ChanDir::SendOnly {
                    let xs = self.operand_str(x);
                    self.error(
                        e.op_pos.0 as u32,
                        Code::InvalidReceive,
                        format!("cannot receive from send-only channel {}", xs),
                    );
                    x.mode = OperandMode::Invalid;
                    return;
                }
                // A receive yields a comma-ok operand: in a 2-valued context
                // (`v, ok := <-ch`) it unpacks to (elem, bool) via eval_multi;
                // in a single-valued context it behaves as a plain value of the
                // element type. (Go: `x.mode = commaok`.)
                x.mode = OperandMode::CommaOk;
                x.typ = Some(crate::chan::chan_elem(&self.types, chan));
                return;
            }
            _ => {}
        }

        // Validate the operator against the operand's type.
        if !unary_op_ok(&mut self.types, &self.objects, &self.packages, op, typ) {
            self.error(
                e.op_pos.0 as u32,
                Code::UndefinedOp,
                format!("operator {:?} not defined on operand", op),
            );
            x.mode = OperandMode::Invalid;
            return;
        }

        // Constant folding.
        if x.mode == OperandMode::Constant {
            match &x.val {
                Some(Value::Unknown) | None => return,
                Some(v) => {
                    // Bitwise complement (`^`) of a *typed unsigned* constant
                    // must be masked to the type's bit width, matching Go's
                    // `constant.UnaryOp(XOR, x, prec)`. Without this, `^uint(0)`
                    // folds to the bignum `-1` instead of `2^64-1`; a later
                    // `-1 >> k` then stays `-1`, and reading it as a shift count
                    // yields `u64::MAX`, producing an astronomically large
                    // shift (e.g. runtime's `1 << (^uintptr(0) >> 63)`), which
                    // exhausts memory. Untyped constants keep prec 0 (arbitrary
                    // precision), as Go does.
                    let prec = if op == Token::XOR && is_unsigned(&self.types, typ) {
                        (self.conf_sizeof(typ) * 8).max(0) as usize
                    } else {
                        0
                    };
                    x.val = Some(unary_op(op, v.clone(), prec));
                    // expr set by raw_expr
                    self.overflow(x, e.op_pos.0 as u32, op_name_unary(op));
                    return;
                }
            }
        }

        x.mode = OperandMode::Value;
        // x.typ remains unchanged.
    }

    /// Type-check a binary expression (`x op y`).
    ///
    /// Equivalent to `Checker.binary`. Comparison and shift are delegated;
    /// constant operands are folded.
    fn binary<'a>(&mut self, x: &mut Operand<'a>, e: &'a BinaryExpr) {
        let mut y = Operand::invalid();
        self.expr(x, &e.x);
        self.expr(&mut y, &e.y);

        if x.mode == OperandMode::Invalid {
            return;
        }
        if y.mode == OperandMode::Invalid {
            x.mode = OperandMode::Invalid;
            x.expr = y.expr;
            return;
        }

        let op = e.op;
        if is_shift_op(op) {
            self.shift(x, &mut y, e, op);
            return;
        }

        self.match_types(x, &mut y);
        if x.mode == OperandMode::Invalid {
            return;
        }

        if is_comparison_op(op) {
            self.comparison(x, &mut y, op, e.op_pos.0 as u32);
            return;
        }

        let xt = x.typ.unwrap_or_else(|| self.invalid_type());
        let yt = y.typ.unwrap_or_else(|| self.invalid_type());
        if !identical(&mut self.types, &self.objects, &self.packages, xt, yt) {
            if is_valid(&self.types, xt) && is_valid(&self.types, yt) {
                let (xs, ys) = (self.type_str(xt), self.type_str(yt));
                self.error(
                    e.op_pos.0 as u32,
                    Code::MismatchedTypes,
                    format!("mismatched types {} and {}", xs, ys),
                );
            }
            x.mode = OperandMode::Invalid;
            return;
        }

        if !binary_op_ok(&mut self.types, &self.objects, &self.packages, op, xt) {
            self.error(
                e.op_pos.0 as u32,
                Code::UndefinedOp,
                format!("operator {:?} not defined on operand", op),
            );
            x.mode = OperandMode::Invalid;
            return;
        }

        // Division/remainder by zero.
        if matches!(op, Token::QUO | Token::REM) {
            let x_intish = x.mode == OperandMode::Constant
                || all_integer(&mut self.types, &self.objects, &self.packages, xt);
            if x_intish && y.mode == OperandMode::Constant {
                if let Some(yv) = &y.val {
                    if sign(yv) == 0 {
                        self.error(e.op_pos.0 as u32, Code::DivByZero, "division by zero");
                        x.mode = OperandMode::Invalid;
                        return;
                    }
                }
            }
        }

        // Constant folding.
        if x.mode == OperandMode::Constant && y.mode == OperandMode::Constant {
            let (xv, yv) = match (&x.val, &y.val) {
                (Some(a), Some(b)) => (a.clone(), b.clone()),
                _ => {
                    x.val = Some(Value::Unknown);
                    return;
                }
            };
            if xv.kind() == guff_constant::Kind::Unknown
                || yv.kind() == guff_constant::Kind::Unknown
            {
                x.val = Some(Value::Unknown);
                return;
            }
            // Force integer division for integer operands (Go's QUO_ASSIGN).
            let fold_op = if op == Token::QUO && is_integer(&self.types, xt) {
                Token::QuoAssign
            } else {
                op
            };
            x.val = Some(binary_op(xv, fold_op, yv));
            // expr set by raw_expr
            self.overflow(x, e.op_pos.0 as u32, op_name_binary(op));
            return;
        }

        x.mode = OperandMode::Value;
        // x.typ is unchanged.
    }

    /// If one operand is untyped, convert it toward the other's type (and vice
    /// versa). Simplified `Checker.matchTypes` — the `mayConvert` guard is
    /// reduced to "attempt only when at least one operand is untyped"; a true
    /// type mismatch surfaces as a conversion error from `convert_untyped`.
    pub(crate) fn match_types<'a>(&mut self, x: &mut Operand<'a>, y: &mut Operand<'a>) {
        let xt = x.typ.unwrap_or_else(|| self.invalid_type());
        let yt = y.typ.unwrap_or_else(|| self.invalid_type());
        if (x.is_nil() && has_nil(&self.types, yt)) || (y.is_nil() && has_nil(&self.types, xt)) {
            return;
        }
        if is_typed(&self.types, xt) && is_typed(&self.types, yt) {
            return;
        }
        self.convert_untyped(x, yt);
        if x.mode == OperandMode::Invalid {
            return;
        }
        let xt_now = x.typ.unwrap_or_else(|| self.invalid_type());
        self.convert_untyped(y, xt_now);
        if y.mode == OperandMode::Invalid {
            x.mode = OperandMode::Invalid;
        }
    }

    /// Type-check a comparison (`x op y`). The result is an untyped boolean.
    ///
    /// Spec: "In any comparison, the first operand must be assignable to the
    /// type of the second operand, or vice versa." Channel direction narrowing
    /// (`chan T` vs `<-chan T`) relies on that assignability check — identity
    /// alone rejects valid comparisons (vault `helper/fairshare`).
    pub(crate) fn comparison<'a>(&mut self, x: &mut Operand<'a>, y: &mut Operand<'a>, op: Token, pos: u32) {
        let xt = x.typ.unwrap_or_else(|| self.invalid_type());
        let yt = y.typ.unwrap_or_else(|| self.invalid_type());
        if !is_valid(&self.types, xt) || !is_valid(&self.types, yt) {
            x.mode = OperandMode::Invalid;
            return;
        }

        // Nil vs typed: covered by assignable_to, but keep the explicit path
        // for the defined-on-operands check below.
        let nil_ok =
            (x.is_nil() && has_nil(&self.types, yt)) || (y.is_nil() && has_nil(&self.types, xt));
        if !nil_ok {
            let ok = self.assignable_to(x, yt).ok || self.assignable_to(y, xt).ok;
            if !ok {
                let (xs, ys) = (self.type_str(xt), self.type_str(yt));
                self.error(
                    pos,
                    Code::MismatchedTypes,
                    format!("mismatched types {} and {}", xs, ys),
                );
                x.mode = OperandMode::Invalid;
                return;
            }
        }

        let defined = match op {
            Token::EQL | Token::NEQ => {
                nil_ok
                    || (comparable(&mut self.types, &self.objects, &self.packages, xt)
                        && comparable(&mut self.types, &self.objects, &self.packages, yt))
            }
            Token::LSS | Token::LEQ | Token::GTR | Token::GEQ => {
                all_ordered(&mut self.types, &self.objects, &self.packages, xt)
                    && all_ordered(&mut self.types, &self.objects, &self.packages, yt)
            }
            _ => false,
        };
        if !defined {
            self.error(
                pos,
                Code::UndefinedOp,
                format!("operator {:?} not defined on operands", op),
            );
            x.mode = OperandMode::Invalid;
            return;
        }

        // Constant folding.
        if x.mode == OperandMode::Constant && y.mode == OperandMode::Constant {
            match (&x.val, &y.val) {
                (Some(a), Some(b)) => {
                    x.val = Some(make_bool(compare(a.clone(), op, b.clone())));
                }
                _ => x.val = Some(Value::Unknown),
            }
            // The operands are never materialized; no need to update them.
        } else {
            x.mode = OperandMode::Value;
            // The operands now have their final (runtime) types. If they are
            // still untyped, that type is the respective default type. Pin them
            // so the recorded types reflect the materialized form.
            let dx = default_type(&self.types, &self.typ, xt);
            let dy = default_type(&self.types, &self.typ, yt);
            if let Some(xe) = x.expr {
                self.update_expr_type(xe, dx, true);
            }
            if let Some(ye) = y.expr {
                self.update_expr_type(ye, dy, true);
            }
        }

        // spec: comparisons yield an untyped boolean value.
        x.typ = Some(self.basic(BasicKind::UntypedBool));
    }

    /// Type-check a shift (`x << y` / `x >> y`).
    ///
    /// Simplified `Checker.shift`: handles the common constant case and the
    /// non-constant value case. The full untyped-lhs / delayed-type machinery
    /// (Go's `updateExprType` interplay) is DEFERRED.
    fn shift<'a>(&mut self, x: &mut Operand<'a>, y: &mut Operand<'a>, e: &BinaryExpr, op: Token) {
        let xt = x.typ.unwrap_or_else(|| self.invalid_type());

        // The shift count must be an integer.
        let yt = y.typ.unwrap_or_else(|| self.invalid_type());
        if !all_integer(&mut self.types, &self.objects, &self.packages, yt) {
            self.error(
                e.op_pos.0 as u32,
                Code::InvalidShiftCount,
                "shift count must be integer",
            );
            x.mode = OperandMode::Invalid;
            return;
        }

        // The left operand must be an integer (or an untyped constant that can
        // become one).
        let x_int_ok = all_integer(&mut self.types, &self.objects, &self.packages, xt)
            || (x.mode == OperandMode::Constant && is_untyped_int_const(&self.types, xt));
        if !x_int_ok {
            self.error(
                e.op_pos.0 as u32,
                Code::InvalidShiftOperand,
                "shifted operand must be integer",
            );
            x.mode = OperandMode::Invalid;
            return;
        }

        if x.mode == OperandMode::Constant && y.mode == OperandMode::Constant {
            let (xv, yv) = match (&x.val, &y.val) {
                (Some(a), Some(b)) => (a.clone(), b.clone()),
                _ => {
                    x.val = Some(Value::Unknown);
                    return;
                }
            };
            let (count, _) = uint64_val(&yv);
            x.val = Some(shift(xv, op, count as u32));
            // expr set by raw_expr
            self.overflow(x, e.op_pos.0 as u32, op_name_binary(Token::SHL));
            // If x was untyped, it stays untyped int here (default applied
            // later). x.typ is unchanged.
            return;
        }

        // Non-constant shift with an untyped constant lhs: per spec the lhs
        // keeps the type it would have on its own, so leave it untyped and mark
        // its node as a shift-lhs operand. When it later materialises, its
        // final type is checked to be an integer (Go's `info.isLhs = true`).
        if x.mode == OperandMode::Constant && is_untyped(&self.types, xt) {
            if let Some(xe) = x.expr {
                if let Some(info) = self.untyped.get_mut(&xe.id()) {
                    info.is_lhs = true;
                }
            }
            x.mode = OperandMode::Value;
            return;
        }

        // Non-constant shift: result has the type of the left operand.
        x.mode = OperandMode::Value;
        // x.typ unchanged.
    }

    /// Resolve an identifier operand.
    ///
    /// Equivalent to `Checker.ident` (`typexpr.go`). `want_type` requests that
    /// the identifier denote a type. The use/dependency-tracking and version
    /// gates are deferred (see module docs).
    pub fn ident<'a>(&mut self, x: &mut Operand<'a>, e: &Ident, want_type: bool) {
        x.mode = OperandMode::Invalid;
        // expr set by raw_expr

        let name = e.name.as_str();
        let (found_scope, obj) = match self.lookup_scope(name) {
            None => {
                let pos = e.pos().0 as u32;
                if name == "_" {
                    self.error(pos, Code::InvalidBlank, "cannot use _ as value or type");
                } else if is_valid_name(name) {
                    self.error(pos, Code::UndeclaredName, format!("undefined: {}", name));
                }
                return;
            }
            Some(o) => o,
        };

        // DEFERRED: verifyVersionf gate for predeclared comparable/any.
        self.record_use(e, obj);
        self.mark_dot_import_use(Some(found_scope), name);

        let is_type_name = matches!(self.objects.get(obj), ObjectData::TypeName(_));

        // If a type is wanted but the object isn't a type name, stop early with
        // a better error (go.dev/issue/65344).
        if !is_type_name && want_type {
            let pos = e.pos().0 as u32;
            let kind = object_kind(self.objects.get(obj));
            self.error(
                pos,
                Code::NotAType,
                format!("{} ({}) is not a type", name, kind),
            );
            return;
        }

        // Type-check the object. Match go/types `Checker.ident`:
        // - `typ == nil` → force objDecl (TypeName / Func before their decl).
        // - Const/Var still at the resolver's Typ[Invalid] placeholder → same
        //   as Go's nil (not yet checked); force objDecl so forward refs like
        //   `var UTC = &utcLoc` see utcLoc's real type.
        // - TypeName from this package when a type is wanted → force for cycle
        //   detection (go.dev/issue/25790).
        let mut typ = obj.typ(&self.objects);
        let is_const_or_var = matches!(
            self.objects.get(obj),
            ObjectData::Const(_) | ObjectData::Var(_)
        );
        let needs_obj_decl = match typ {
            None => true,
            Some(t)
                if is_const_or_var && t == self.invalid_type() =>
            {
                true
            }
            Some(_)
                if is_type_name
                    && want_type
                    && obj.pkg(&self.objects) == Some(self.pkg) =>
            {
                true
            }
            _ => false,
        };
        if needs_obj_decl {
            self.obj_decl(obj);
            typ = obj.typ(&self.objects);
        }
        let typ = match typ {
            Some(t) => t,
            None => self.invalid_type(),
        };

        // Record a dependency edge from the current package-level declaration
        // to this constant/variable/function (drives init_order). Done before
        // the fill match because that match holds an immutable borrow of
        // `self.objects` while `add_decl_dep` needs `&mut self`.
        if matches!(
            self.objects.get(obj),
            ObjectData::Const(_) | ObjectData::Var(_) | ObjectData::Func(_)
        ) {
            self.add_decl_dep(obj);
        }

        // Fill the operand based on the object kind.
        match self.objects.get(obj) {
            ObjectData::Const(c) => {
                let cval = c.val().clone();
                if !is_valid(&self.types, typ) {
                    x.typ = Some(typ);
                    return;
                }
                // iota special-case: a reference to the predeclared `iota`
                // uses the current const-block iota value.
                if is_iota(name, obj, self) {
                    match &self.env.iota {
                        Some(v) => x.val = Some(v.clone()),
                        None => {
                            let pos = e.pos().0 as u32;
                            self.error(
                                pos,
                                Code::InvalidIota,
                                "cannot use iota outside constant declaration",
                            );
                            return;
                        }
                    }
                } else {
                    x.val = Some(cval);
                }
                x.mode = OperandMode::Constant;
            }
            ObjectData::TypeName(_) => {
                x.mode = OperandMode::TypeExpr;
            }
            ObjectData::Var(_) => {
                // Mark the variable used (so it doesn't trigger a
                // "declared and not used" error). Ignore variables from other
                // packages, matching Go. (addDeclDep is handled above.)
                if obj.pkg(&self.objects) == Some(self.pkg) {
                    self.used_vars.insert(obj);
                }
                if !is_valid(&self.types, typ) {
                    x.typ = Some(typ);
                    return;
                }
                x.mode = OperandMode::Variable;
            }
            ObjectData::Func(_) => {
                x.mode = OperandMode::Value;
            }
            ObjectData::Builtin(b) => {
                x.id = Some(b.id());
                x.mode = OperandMode::Builtin;
            }
            ObjectData::Nil(_) => {
                x.mode = OperandMode::NilValue;
            }
            ObjectData::PkgName(_) => {
                // A package name can only appear in a qualified identifier
                // (`pkg.X`), which `selector` handles before checking `e.X`.
                // Reaching here means a bare package name used as a value.
                let pos = e.pos().0 as u32;
                self.error(
                    pos,
                    Code::InvalidPkgUse,
                    format!("use of package {} not in selector", name),
                );
                return;
            }
        }

        x.typ = Some(typ);
    }
}

/// Reports whether `op` is a shift operator (`<<` / `>>`).
fn is_shift_op(op: Token) -> bool {
    matches!(op, Token::SHL | Token::SHR)
}

/// Reports whether `op` is a comparison operator.
fn is_comparison_op(op: Token) -> bool {
    matches!(
        op,
        Token::EQL | Token::NEQ | Token::LSS | Token::LEQ | Token::GTR | Token::GEQ
    )
}

/// Reports whether binary operator `op` is defined on operands of type `t`.
/// Mirrors `binaryOpPredicates` (non-shift, non-comparison ops).
///
/// The predicates are the type-set-aware `allX` family, so a type parameter
/// whose constraint admits only numeric terms supports `+` and friends.
/// The operator names Go's `opName` puts into a "constant … overflow" message.
/// Anything absent from Go's two tables yields `""`, which drops the word.
fn op_name_unary(op: Token) -> &'static str {
    match op {
        Token::XOR => "bitwise complement",
        _ => "",
    }
}

fn op_name_binary(op: Token) -> &'static str {
    match op {
        Token::ADD => "addition",
        Token::SUB => "subtraction",
        Token::XOR => "bitwise XOR",
        Token::MUL => "multiplication",
        Token::SHL => "shift",
        _ => "",
    }
}

fn binary_op_ok(
    arena: &mut crate::arena::TypeArena,
    objects: &crate::arena::ObjectArena,
    packages: &crate::arena::PackageArena,
    op: Token,
    t: crate::TypeId,
) -> bool {
    match op {
        Token::ADD => all_numeric_or_string(arena, objects, packages, t),
        Token::SUB | Token::MUL | Token::QUO => all_numeric(arena, objects, packages, t),
        Token::REM | Token::AND | Token::OR | Token::XOR | Token::AndNot => {
            all_integer(arena, objects, packages, t)
        }
        Token::LAND | Token::LOR => all_boolean(arena, objects, packages, t),
        _ => false,
    }
}

/// Reports whether `t` is the untyped-int/rune basic type (used to permit an
/// untyped constant on the left of a shift).
fn is_untyped_int_const(arena: &crate::arena::TypeArena, t: crate::TypeId) -> bool {
    matches!(
        arena.get(t.underlying(arena)),
        crate::arena::TypeData::Basic(b) if matches!(b.kind(), BasicKind::UntypedInt | BasicKind::UntypedRune)
    )
}

/// Reports whether unary operator `op` is defined on an operand of type `t`.
/// Mirrors the relevant entries of `unaryOpPredicates`:
/// `+`/`-` require a numeric type, `^` an integer, `!` a boolean —
/// type-set-aware, as upstream's are.
fn unary_op_ok(
    arena: &mut crate::arena::TypeArena,
    objects: &crate::arena::ObjectArena,
    packages: &crate::arena::PackageArena,
    op: Token,
    t: crate::TypeId,
) -> bool {
    match op {
        Token::ADD | Token::SUB => all_numeric(arena, objects, packages, t),
        Token::XOR => all_integer(arena, objects, packages, t),
        Token::NOT => all_boolean(arena, objects, packages, t),
        _ => false,
    }
}

/// Reports whether `name` is a valid identifier name (non-empty, not the
/// numeric blank placeholder). Mirrors `isValidName`.
fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && name != "_"
}

/// Reports whether the object referenced by `name` is the predeclared `iota`.
fn is_iota(name: &str, obj: crate::ObjectId, check: &Checker) -> bool {
    if name != "iota" {
        return false;
    }
    // Predeclared iota lives directly in the universe scope.
    obj.parent(&check.objects) == Some(check.universe_scope)
}

/// A short human-readable object-kind label for error messages. Mirrors the
/// relevant cases of `objectKind`.
fn object_kind(obj: &ObjectData) -> &'static str {
    match obj {
        ObjectData::Const(_) => "constant",
        ObjectData::TypeName(_) => "type",
        ObjectData::Var(_) => "variable",
        ObjectData::Func(_) => "func",
        ObjectData::Builtin(_) => "builtin",
        ObjectData::Nil(_) => "nil",
        ObjectData::PkgName(_) => "package",
    }
}
