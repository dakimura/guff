//! Port of selector resolution from `cmd/compile/internal/types2/call.go`.
//!
//! **Chunk 26**: [`Checker::selector`] — resolves `x.f` (field selection,
//! method value, and method expression). The call body (`callExpr` /
//! `arguments` / generic inference) lands in chunk 27; index/slice in chunk 28.
//!
//! ## Deferrals (chunk-26, see §8)
//!
//! - **Package selectors `pkg.X`** (D16): Go handles a qualified identifier
//!   whose `x.X` resolves to a `*PkgName` entirely up front. We have no
//!   `PkgName` object kind and no importer, so that branch is omitted — a
//!   package-qualified selector currently fails as an undefined identifier
//!   when `expr` checks `e.x`. The `cgo` special-cases are likewise dropped.
//! - **`recordSelection`** populates `Info.Selections` (chunk 53); its
//!   `recordUse(x.Sel, obj)` side effect is part of it. `addDeclDep`
//!   (dependency tracking) is wired separately (chunk 48).
//! - Error diagnostics are simplified: `interfacePtrError` / `lookupError`
//!   produce a short "(no field or method ...)" reason rather than Go's
//!   "wrong case"/"pointer receiver" hints.

use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff_types_errors::Code;

use crate::arena::{ObjectData, TypeData, TypeId};
use crate::check::Checker;
use crate::infer::{infer, rename_tparams, InferResult};
use crate::instantiate::instantiate;
use crate::lookup::{is_interface_ptr, lookup_field_or_method, LookupResult};
use crate::object::builtin::{ExprKind, PREDECLARED_FUNCS};
use crate::object::var::new_param;
use crate::operand::{Operand, OperandMode};
use crate::predicates::{is_untyped, is_valid};
use crate::scope::{lookup as scope_lookup, lookup_ignoring_case};
use crate::selection::SelectionKind;
use crate::signature::new_signature_type;
use crate::slice::slice_elem;
use crate::tuple::{new_tuple, tuple_at, tuple_len};
use crate::under::common_under;

impl Checker {
    /// Type-check a call expression `fun(args...)`, recording the result in `x`.
    ///
    /// Equivalent to `Checker.callExpr` (chunk-27 subset). Three forms are
    /// distinguished by the mode of `fun`:
    ///
    /// - **conversion** (`fun` is a type, `T(x)`) — single-argument conversion
    ///   via the in-place [`Checker::conversion`] driver (constant folding,
    ///   representability rounding, untyped final-type update; D09 recovered).
    /// - **builtin call** — dispatched to [`Checker::builtin`] (`builtins.rs`).
    /// - **ordinary function/method call** — count-checks the arguments and
    ///   `assignment`-checks each against its parameter type, then sets the
    ///   result from the signature's results tuple.
    ///
    /// ## Deferrals (chunk-27, see §8)
    ///
    /// - **Generic calls** (callee signature with type parameters) — the
    ///   `genericExprList` / `renameTParams` / `infer` plumbing is not wired
    ///   in; such a call leaves the operand invalid (D21).
    /// - **Explicit generic function instantiation as `fun`** (`f[int](...)` /
    ///   `f[T1, T2](...)`) — handled up front via [`Checker::func_inst`] (chunk
    ///   71); the resulting concrete signature flows through the ordinary path.
    /// - **Multi-valued single argument** (`f(g())` where `g` is multi-valued)
    ///   — `genericExprList`'s spreading is not ported; each argument is a
    ///   single value.
    /// - `hasCallOrRecv` / `record` / reverse type inference are omitted.
    ///
    /// Returns the [`ExprKind`] of the call (Go's `callExpr` result): a
    /// conversion, an expression (expression-valued builtin), or a statement
    /// (ordinary call, or statement-only builtin). Callers use it to decide
    /// whether the call is legal in statement position (`ExprStmt`, `go`,
    /// `defer`).
    pub fn call_expr<'a>(&mut self, x: &mut Operand<'a>, call_e: &'a Expr) -> ExprKind {
        let call = match call_e {
            Expr::CallExpr(c) => c,
            _ => panic!("call_expr: expected CallExpr"),
        };
        // Evaluate the callee. Explicit generic function instantiation in call
        // position — `f[targs](args)` — is handled specially: `index_expr`
        // signals a generic-function operand and `func_inst` instantiates the
        // signature so the ordinary call path below sees a concrete signature.
        // Type arguments written explicitly but not completely — `f[int](x)`
        // where `f` has two type parameters. Upstream leaves the signature
        // generic in that case and carries the partial list into `arguments`,
        // where one `infer` sees both what was written and what the arguments
        // imply (`callExpr` → `arguments(call, sig, targs, …)`).
        let mut partial_targs: Vec<TypeId> = Vec::new();
        match call.fun.as_ref() {
            Expr::IndexExpr(ie) => {
                if self.index_expr(x, ie) {
                    partial_targs = self.func_inst(
                        x,
                        &ie.x,
                        std::slice::from_ref(&*ie.index),
                        ie.x.pos().0 as u32,
                        false,
                    );
                }
                // Otherwise `index_expr` already fully evaluated `x` (ordinary
                // indexing yielding a function value, or an invalid operand).
            }
            Expr::IndexListExpr(ie) => {
                let mark = self.errors.len();
                self.expr(x, &ie.x);
                if self.is_generic_func_value(x) {
                    partial_targs =
                        self.func_inst(x, &ie.x, &ie.indices, ie.x.pos().0 as u32, false);
                } else if x.mode == OperandMode::TypeExpr {
                    // `T[A, B](v)` — a conversion to an instantiated generic
                    // *type*, not a call. The single-argument form reaches this
                    // through `index_expr`, which instantiates; the multi-index
                    // form had only the generic-function branch, so
                    // `iter.Seq2[[]T, error](fn)` was reported as "cannot index
                    // iter.Seq2[K, V any]" and took its package with it. Nine of
                    // jaeger's packages are that one line.
                    self.errors.truncate(mark);
                    self.expr_or_type(x, &call.fun);
                } else if x.mode != OperandMode::Invalid {
                    // A multi-index on a non-generic operand is not a valid call
                    // target (ordinary indexing takes a single index).
                    let xs = self.operand_str(x);
                    self.error(
                        ie.x.pos().0 as u32,
                        Code::NonSliceableOperand,
                        format!("cannot index {}", xs),
                    );
                    x.mode = OperandMode::Invalid;
                    x.typ = Some(self.invalid_type());
                }
            }
            _ => {
                // A conversion's operand may be written as a parenthesized type
                // (`(*T)(x)`) or as a bare type literal (`[]byte(s)`,
                // `map[string]int(m)`, `interface{}(v)`, `struct{...}(v)`,
                // `chan int(c)`, `func()(f)`). None of those forms are
                // expressions, so `expr` would leave the operand invalid — and
                // an invalid function operand is swallowed silently below,
                // which used to drop the whole conversion (no type recorded, no
                // diagnostics). Route every syntactic type form through
                // `expr_or_type`; anything else is evaluated as a value, so a
                // non-type operand still gets its normal error.
                if is_type_syntax(call.fun.as_ref()) {
                    self.expr_or_type(x, &call.fun);
                } else {
                    self.expr(x, &call.fun);
                }
            }
        }

        match x.mode {
            OperandMode::Invalid => {
                self.use_args(&call.args);
                x.expr = Some(call_e);
                return ExprKind::Statement;
            }
            OperandMode::TypeExpr => {
                // Conversion `T(x)`.
                let t = x.typ.unwrap_or_else(|| self.invalid_type());
                x.mode = OperandMode::Invalid;
                let has_dots = crate::util::has_dots(call);
                match call.args.len() {
                    0 => {
                        let ts = self.type_str(t);
                        self.error(
                            call.pos().0 as u32,
                            Code::WrongArgCount,
                            format!("missing argument in conversion to {}", ts),
                        );
                    }
                    1 => {
                        let mut arg = Operand::invalid();
                        self.expr(&mut arg, &call.args[0]);
                        if arg.mode != OperandMode::Invalid {
                            if has_dots {
                                let ts = self.type_str(t);
                                self.error(
                                    call.args[0].pos().0 as u32,
                                    Code::BadDotDotDotSyntax,
                                    format!("invalid use of ... in conversion to {}", ts),
                                );
                            } else {
                                // The in-place conversion driver folds constants
                                // (representability rounding, integer→string),
                                // reports `InvalidConversion`, and updates the
                                // argument's recorded untyped type.
                                self.conversion(&mut arg, t);
                                x.mode = arg.mode;
                                x.typ = arg.typ;
                                x.val = arg.val;
                            }
                        }
                    }
                    n => {
                        self.use_args(&call.args);
                        let ts = self.type_str(t);
                        self.error(
                            call.args[n - 1].pos().0 as u32,
                            Code::WrongArgCount,
                            format!("too many arguments in conversion to {}", ts),
                        );
                    }
                }
                x.expr = Some(call_e);
                return ExprKind::Conversion;
            }
            OperandMode::Builtin => {
                let id = x.id.expect("builtin operand carries a BuiltinId");
                if !self.builtin(x, call, id) {
                    x.mode = OperandMode::Invalid;
                }
                x.expr = Some(call_e);
                // Go returns `predeclaredFuncs[id].kind` (expression or
                // statement) regardless of success.
                return PREDECLARED_FUNCS[id as usize].kind;
            }
            _ => {}
        }

        // Ordinary function/method call.
        let ftyp = x.typ.unwrap_or_else(|| self.invalid_type());
        // If the callee's type is a type parameter, every type in its type set
        // must share one underlying type and that type must be a signature —
        // `func New[T any, F func(Conn) T](fn F, c Conn) T { return fn(c) }`.
        // `commonUnder` collapses the type set to that signature; for every
        // other callee it is just the underlying type.
        let under = {
            let (u, _err) = common_under(
                &mut self.types,
                &self.objects,
                &self.packages,
                ftyp,
                None,
            );
            u.unwrap_or_else(|| ftyp.underlying(&self.types))
        };
        let sig = match self.types.get(under) {
            TypeData::Signature(_) => under,
            _ => {
                if is_valid(&self.types, ftyp) {
                    let s = self.type_str(ftyp);
                    self.error(
                        call.pos().0 as u32,
                        Code::InvalidCall,
                        format!("cannot call non-function (of type {})", s),
                    );
                }
                self.use_args(&call.args);
                x.mode = OperandMode::Invalid;
                x.typ = Some(self.invalid_type());
                x.expr = Some(call_e);
                return ExprKind::Statement;
            }
        };

        // Check arguments. For a generic callee, `arguments` infers the type
        // arguments from the call's argument types and returns the instantiated
        // (non-generic) signature; for an ordinary callee it returns `sig`.
        let sig = match self.arguments(call, sig, &partial_targs) {
            Some(s) => s,
            None => {
                x.mode = OperandMode::Invalid;
                x.typ = Some(self.invalid_type());
                x.expr = Some(call_e);
                return ExprKind::Statement;
            }
        };

        // Determine the result.
        let results = match self.types.get(sig) {
            TypeData::Signature(s) => s.results(),
            _ => None,
        };
        match tuple_len(&self.types, results) {
            0 => x.mode = OperandMode::NoValue,
            1 => {
                x.mode = OperandMode::Value;
                let r = tuple_at(&self.types, results.unwrap(), 0);
                x.typ = r.typ(&self.objects);
            }
            _ => {
                x.mode = OperandMode::Value;
                x.typ = results;
            }
        }
        x.expr = Some(call_e);
        ExprKind::Statement
    }

    /// Reports whether operand `x` is a generic function value — a `Value`
    /// whose (underlying) type is a `Signature` with type parameters. Such an
    /// operand is the base of an explicit instantiation `f[targs]`.
    pub(crate) fn is_generic_func_value(&self, x: &Operand) -> bool {
        if x.mode != OperandMode::Value {
            return false;
        }
        let t = match x.typ {
            Some(t) => t,
            None => return false,
        };
        let u = t.underlying(&self.types);
        matches!(
            self.types.get(u),
            TypeData::Signature(s) if s.type_params().map_or(0, |l| l.len()) > 0
        )
    }

    /// Type-check an explicit generic function instantiation `f[targs...]`.
    ///
    /// `x` holds the generic function value; `base` is the instantiated
    /// expression (`f`) for `Info.Instances` recording; `index_exprs` are the
    /// explicit type-argument expressions. On success `x` becomes the
    /// instantiated function value (mode `Value`, a concrete non-generic
    /// `Signature`), so a surrounding call flows through the ordinary path.
    ///
    /// Equivalent to `Checker.funcInst`. `infer` is upstream's parameter of the
    /// same name: with it false — the call-position caller — a *partial*
    /// explicit instantiation (`got < want`) is not an error here. The type
    /// arguments that were written are returned and `x` keeps its generic
    /// signature, so [`Checker::arguments`] can hand them and the argument
    /// types to one `infer`. The returned vector is empty in every other case
    /// (fully instantiated, invalid, or `infer` true).
    ///
    /// **Deferred**: inference from an assignment target (Go's `target`
    /// machinery, `var f func(int) = g` for generic `g`) — with `infer` true
    /// and `got < want` this still reports `CannotInferTypeArgs`, which is what
    /// upstream does when it has no target either.
    pub(crate) fn func_inst(
        &mut self,
        x: &mut Operand,
        base: &Expr,
        index_exprs: &[Expr],
        pos: u32,
        infer: bool,
    ) -> Vec<TypeId> {
        // Go: verifyVersionf(go1_18, "function instantiation") — gate deferred.

        // A function value's type is a Signature directly.
        let sig = x.typ.unwrap_or_else(|| self.invalid_type());
        let tparams: Vec<TypeId> = match self.types.get(sig) {
            TypeData::Signature(s) => s
                .type_params()
                .map(|l| l.list().to_vec())
                .unwrap_or_default(),
            _ => Vec::new(),
        };

        // Evaluate the explicit type arguments; bail if any is invalid.
        let targs = match self.type_list(index_exprs) {
            Some(t) => t,
            None => {
                x.mode = OperandMode::Invalid;
                x.typ = Some(self.invalid_type());
                return Vec::new();
            }
        };

        let (got, want) = (targs.len(), tparams.len());
        if got > want {
            let at = index_exprs
                .get(got - 1)
                .map(|e| e.pos().0 as u32)
                .unwrap_or(pos);
            self.error(
                at,
                Code::WrongTypeArgCount,
                format!("got {} type arguments but want {}", got, want),
            );
            x.mode = OperandMode::Invalid;
            x.typ = Some(self.invalid_type());
            return Vec::new();
        }
        if got < want {
            if !infer {
                // Call position: `x` stays the generic function and the caller
                // completes the list from the argument types. `sets.KeySet[string](m)`
                // is the shape — one of two type parameters written, the other
                // only knowable from `m` — and it cost kubernetes' `util/sets`
                // and everything importing it.
                return targs;
            }
            // No call to learn the rest from, and the assignment-target path is
            // not ported: same diagnostic upstream gives when `infer` finds
            // nothing to work with.
            self.error(
                pos,
                Code::CannotInferTypeArgs,
                format!(
                    "cannot infer the remaining type arguments (got {}, want {})",
                    got, want
                ),
            );
            x.mode = OperandMode::Invalid;
            x.typ = Some(self.invalid_type());
            return Vec::new();
        }

        // got == want: verify constraints (soft error) and instantiate.
        if let Some((i, cause)) = self.verify_targs(&tparams, &targs) {
            let at = index_exprs.get(i).map(|e| e.pos().0 as u32).unwrap_or(pos);
            self.error(at, Code::InvalidTypeArg, cause);
        } else {
            self.mono.record_instance(
                &self.types,
                &self.objects,
                &self.scopes,
                &self.packages,
                self.pkg,
                pos,
                &tparams,
                &targs,
                index_exprs,
            );
        }

        let inst = instantiate(
            &mut self.types,
            &mut self.objects,
            &mut self.ctxt,
            sig,
            targs.clone(),
        );
        // recordInstance: map the instantiated identifier (`f`) to its explicit
        // type arguments and the resulting concrete signature.
        self.record_instance(base, targs, inst);
        x.typ = Some(inst);
        x.mode = OperandMode::Value;
        Vec::new()
    }

    /// Count-check and `assignment`-check a call's arguments against `sig`,
    /// returning the (possibly instantiated) signature to read results from, or
    /// `None` (after reporting an error) on a count / `...` / inference failure.
    ///
    /// Simplified `Checker.arguments`. For a **generic callee** the type
    /// arguments are inferred from the argument types via [`crate::infer`] and
    /// the signature is instantiated. `partial_targs` are the type arguments
    /// the call wrote out explicitly when it did not write them all
    /// (`f[int](x)` for a two-parameter `f`); they seed inference in the same
    /// positions upstream seeds them. Multi-valued single arguments are not
    /// handled (see [`Checker::call_expr`] deferrals). Variadic spreading
    /// builds the per-argument target type inline.
    pub(crate) fn arguments(
        &mut self,
        call: &CallExpr,
        sig: TypeId,
        partial_targs: &[TypeId],
    ) -> Option<TypeId> {
        let (params, variadic) = match self.types.get(sig) {
            TypeData::Signature(s) => (s.params(), s.variadic()),
            _ => return None,
        };
        let npars = tuple_len(&self.types, params);
        let nargs = call.args.len();
        let ddd = call.ellipsis.0 != 0;

        // Evaluate every argument (so all are "used" even on a later error).
        // A lone argument goes through `raw_expr`, since a multi-valued call
        // spread across the parameters is legal there and `expr`'s
        // `single_value` would reject the tuple first. Go's `genericExprList`
        // splits on exactly the same `n == 1` boundary: the single-element arm
        // calls `rawExpr` and unpacks a tuple, every other arm goes through
        // `genericExpr`, which ends in `singleValue`.
        let mut args: Vec<Operand> = Vec::with_capacity(nargs);
        for a in &call.args {
            let mut op = Operand::invalid();
            if nargs == 1 {
                self.raw_expr(&mut op, a, None);
            } else {
                self.expr(&mut op, a);
            }
            args.push(op);
        }

        // A lone multi-valued argument is spread across the parameters:
        // `mux.Handle(newHandler(x))` where `newHandler` returns
        // `(string, http.Handler)`. Go does this in `exprList`/`multiExpr`;
        // without it the call looks like it has one argument too few.
        if args.len() == 1 && !ddd {
            if let Some(t) = args[0].typ {
                if matches!(self.types.get(t), TypeData::Tuple(_)) {
                    let n = tuple_len(&self.types, Some(t));
                    let src = args[0].expr;
                    let mut spread = Vec::with_capacity(n);
                    for i in 0..n {
                        let v = tuple_at(&self.types, t, i);
                        let mut op = Operand::invalid();
                        op.mode = OperandMode::Value;
                        op.typ = v.typ(&self.objects);
                        op.expr = src;
                        spread.push(op);
                    }
                    args = spread;
                }
            }
        }
        let nargs = args.len();

        // `...` is only valid in a call to a variadic function.
        if ddd && !variadic {
            self.error(
                call.pos().0 as u32,
                Code::NonVariadicDotDotDot,
                "cannot use ... in call to non-variadic function".to_string(),
            );
            return None;
        }

        // Argument count requirements (see the table in Go's `arguments`).
        let count_ok = if variadic {
            if ddd {
                nargs == npars
            } else {
                nargs + 1 >= npars
            }
        } else {
            nargs == npars
        };
        if !count_ok {
            let qualifier = if nargs > npars {
                "too many"
            } else {
                "not enough"
            };
            self.error(
                call.pos().0 as u32,
                Code::WrongArgCount,
                format!("{} arguments in call", qualifier),
            );
            return None;
        }

        // For a generic callee, infer the type arguments from the argument
        // types and instantiate the signature.
        let tparam_ids: Vec<TypeId> = match self.types.get(sig) {
            TypeData::Signature(s) => s
                .type_params()
                .map(|l| l.list().to_vec())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let rsig = if !tparam_ids.is_empty() {
            match self.infer_call(call, sig, &tparam_ids, &mut args, nargs, ddd, partial_targs) {
                Some(s) => s,
                None => return None,
            }
        } else {
            sig
        };

        // Check each argument against its parameter type (of the resulting,
        // possibly instantiated, signature).
        let (rparams, rvariadic) = match self.types.get(rsig) {
            TypeData::Signature(s) => (s.params(), s.variadic()),
            _ => (params, variadic),
        };
        let rnpars = tuple_len(&self.types, rparams);
        for i in 0..nargs {
            let ptyp = self.param_type(rparams, rnpars, rvariadic, ddd, i);
            self.assignment(&mut args[i], Some(ptyp), "argument to call");
        }
        Some(rsig)
    }

    /// Infer the type arguments of a generic callee `sig` from the call's
    /// argument operands and return the instantiated (non-generic) signature.
    /// Reports `CannotInferTypeArgs` and returns `None` on failure.
    ///
    /// Equivalent to the inference slice of `Checker.arguments`. `partial_targs`
    /// are the callee's explicitly written type arguments when the call wrote
    /// some but not all of them; they occupy the first positions of the list
    /// `infer` starts from, exactly as upstream's `for len(targs) < len(tparams)
    /// { targs = append(targs, nil) }` leaves them. **Deferred**:
    /// untyped-argument default-type promotion (the `infer` step-3 carry, D11).
    /// Reverse inference from generic function arguments is handled below; a
    /// generic argument's operand type is rewritten in place to the
    /// instantiated signature, as upstream does before it checks the
    /// assignments.
    #[allow(clippy::too_many_arguments)]
    fn infer_call(
        &mut self,
        call: &CallExpr,
        sig: TypeId,
        tparam_ids: &[TypeId],
        args: &mut [Operand],
        nargs: usize,
        ddd: bool,
        partial_targs: &[TypeId],
    ) -> Option<TypeId> {
        // Build the parameter tuple matching the call's argument count
        // (variadic functions need their tail expanded — `infer` requires
        // `params.len() == args.len()`).
        let infer_params = self.call_param_tuple(sig, nargs, ddd);

        // Rename type parameters to avoid problems with recursive generic calls
        // (Go: `arguments` → `renameTParams` before `infer`).
        let (renamed_tparams, renamed_infer_params) = match infer_params {
            Some(p) => {
                let (tp, params) =
                    rename_tparams(&mut self.types, &mut self.objects, tparam_ids, p);
                (tp, Some(params))
            }
            None => {
                let (tp, _) = rename_tparams(&mut self.types, &mut self.objects, tparam_ids, sig);
                (tp, None)
            }
        };

        // Argument types feed step-1 unification (None for invalid operands →
        // skipped by `infer`). Untyped, non-nil constants are withheld from
        // step 1 (an untyped value can only match a single type parameter) and
        // supplied separately via `untyped_types` so `infer`'s step 3 can pick
        // their default type.
        let mut arg_types: Vec<Option<TypeId>> = Vec::with_capacity(args.len());
        let mut untyped_types: Vec<Option<TypeId>> = Vec::with_capacity(args.len());
        for a in args.iter() {
            let untyped = a.typ.is_some_and(|t| is_untyped(&self.types, t));
            if a.mode == OperandMode::Invalid {
                arg_types.push(None);
                untyped_types.push(None);
            } else if untyped {
                // Go's step-1 guard is `isTyped(arg.typ)`, not "is an untyped
                // *constant*". An untyped `nil` is a value, not a constant, so
                // testing the mode let it through to unification, where a
                // parameter type mentioning a type parameter cannot possibly
                // unify with untyped nil — and inference failed outright rather
                // than learning the type parameter from another argument.
                //
                // `source.Kind(ic, &corev1.Pod{}, nil)` is the shape: `object`
                // is right there in argument 2, and argument 3 sank the call.
                arg_types.push(None);
                // Step 3 promotes an untyped argument to its default type, and
                // untyped nil has none — Go excludes it explicitly
                // (`!arg.isNil()`), so it contributes nothing to either step.
                untyped_types.push((a.mode == OperandMode::Constant).then_some(a.typ).flatten());
            } else {
                arg_types.push(a.typ);
                untyped_types.push(None);
            }
        }

        // Interface inference reads the *arguments'* method sets: unifying a
        // parameter `Iface[R]` against a concrete argument matches their methods
        // and learns `R` from the signatures. For a generic instance that only
        // works once the instance's methods have been substituted, and `infer`
        // is a fourth entry point into a method-set comparison that has to do
        // that itself — see `prepare_method_set` for why guff needs the step at
        // all and Go does not.
        //
        // It only shows with *two* arguments naming the same type parameter. One
        // argument leaves the parameter free, so unifying against the origin's
        // own `R` still binds something that happens to be right; the second
        // arrives with `R` already resolved and compares a substituted signature
        // against an unsubstituted one. Both print the same, and the error is
        // `cannot infer type arguments in call` — 24 of them across
        // controller-runtime, and none in any fixture with a one-argument call.
        for t in arg_types.iter().flatten() {
            self.prepare_method_set(*t);
        }

        // --- Reverse type inference (go1.21) -----------------------------
        //
        // An argument that is itself an *uninstantiated generic function* has
        // type parameters of its own, and the callee's type arguments can only
        // be inferred jointly with them:
        //
        //     func each[T any](xs []T, match MatchFunc[T]) {}
        //     func SemanticDeepEqual[U any](a, b U) bool { … }
        //     each([]S{…}, SemanticDeepEqual)   // T and U inferred together
        //
        // Upstream (`Checker.arguments`) clones each such argument signature,
        // renames its type parameters so `f(g, g)` gives the two `g`s distinct
        // identities, appends them to the callee's list, and hands the whole
        // problem to one `infer`. Without it every call of this shape fails —
        // kubernetes' `pkg/api/validate` had 21, all in one file, and lost the
        // package to them.
        let mut all_tparams = renamed_tparams.clone();
        let callee_ntparams = renamed_tparams.len();
        let mut generic_args: Vec<usize> = Vec::new();
        if self.allow_version(&crate::version::go1_21()) {
            for i in 0..arg_types.len() {
                let Some(at) = arg_types[i] else { continue };
                // A generic argument cannot have a defined (*Named) type, so
                // there is no underlying() step here — as upstream notes.
                let atparams: Vec<TypeId> =
                    match crate::signature::signature_type_params(&self.types, at) {
                        Some(l) if !l.is_empty() => l.list().to_vec(),
                        _ => continue,
                    };
                let (new_tparams, renamed) =
                    rename_tparams(&mut self.types, &mut self.objects, &atparams, at);
                // `rename_tparams` does not touch the signature's own tparam
                // list, so re-point it at the fresh parameters.
                crate::signature::signature_set_type_params(
                    &mut self.types,
                    renamed,
                    crate::typelists::TypeParamList::from_bound(new_tparams.clone()),
                );
                arg_types[i] = Some(renamed);
                all_tparams.extend(new_tparams);
                generic_args.push(i);
            }
        }
        let renamed_tparams = all_tparams;

        // The callee's own type parameters come first, so the explicitly
        // written arguments land on the parameters they were written for; the
        // rest — the callee's unwritten ones and every generic argument's —
        // stay open for inference.
        let mut targs_in: Vec<Option<TypeId>> = vec![None; renamed_tparams.len()];
        for (i, t) in partial_targs.iter().take(callee_ntparams).enumerate() {
            targs_in[i] = Some(*t);
        }
        let typ_table = self.typ.clone();
        // Go enables shared-method interface inference for go1.21+ (an unset
        // language version defaults to current, so this is on by default).
        let enable_iface_inference = self.allow_version(&crate::version::go1_21());
        let result = infer(
            &mut self.types,
            &mut self.objects,
            &self.packages,
            &renamed_tparams,
            &targs_in,
            renamed_infer_params,
            &arg_types,
            &untyped_types,
            &typ_table,
            enable_iface_inference,
        );
        match result {
            InferResult::Ok(targs) => {
                // The first `callee_ntparams` entries belong to the callee; the
                // rest were contributed by generic function arguments and are
                // used to instantiate those, not this signature.
                // Instantiate each generic function argument with the type
                // arguments inferred for *its* parameters and write the result
                // back onto the operand, so the assignment check that follows
                // compares `func(S, S) bool` against `MatchFunc[S]` rather than
                // the uninstantiated `func[T any](T, T) bool`.
                let mut j = callee_ntparams;
                for &i in &generic_args {
                    let Some(at) = arg_types[i] else { continue };
                    let k = j + crate::signature::signature_type_params(&self.types, at)
                        .map(|l| l.len())
                        .unwrap_or(0);
                    let atargs: Vec<TypeId> = targs[j..k].to_vec();
                    let inst_arg = instantiate(
                        &mut self.types,
                        &mut self.objects,
                        &mut self.ctxt,
                        at,
                        atargs,
                    );
                    args[i].typ = Some(inst_arg);
                    j = k;
                }
                let callee_targs: Vec<TypeId> =
                    targs.iter().copied().take(callee_ntparams).collect();
                let targs = callee_targs;
                let inst = instantiate(
                    &mut self.types,
                    &mut self.objects,
                    &mut self.ctxt,
                    sig,
                    targs.clone(),
                );
                // Record in the monomorphization flow graph. Go gates this on
                // the deferred `verify` success; the inference path here does
                // not run an explicit verify, so we record unconditionally
                // (a successful inference implies the constraints hold).
                self.mono.record_instance(
                    &self.types,
                    &self.objects,
                    &self.scopes,
                    &self.packages,
                    self.pkg,
                    call.pos().0 as u32,
                    tparam_ids,
                    &targs,
                    &[],
                );
                // recordInstance: map the inferred callee identifier (`F` in
                // `F(args)`) to its inferred type arguments and instantiated
                // signature.
                self.record_instance(&call.fun, targs, inst);
                Some(inst)
            }
            InferResult::Failed(_) => {
                self.error(
                    call.pos().0 as u32,
                    Code::CannotInferTypeArgs,
                    "cannot infer type arguments in call".to_string(),
                );
                None
            }
        }
    }

    /// Build the parameter tuple for a call, expanding a variadic tail so it
    /// has exactly `nargs` entries (mirrors Go's `sigParams` adjustment). For
    /// non-variadic calls or `f(s...)` spreads, the signature's own parameter
    /// tuple is returned unchanged.
    fn call_param_tuple(&mut self, sig: TypeId, nargs: usize, ddd: bool) -> Option<TypeId> {
        let (params, variadic) = match self.types.get(sig) {
            TypeData::Signature(s) => (s.params(), s.variadic()),
            _ => return None,
        };
        let npars = tuple_len(&self.types, params);
        if !variadic || ddd || npars == 0 || nargs < npars - 1 {
            return params;
        }
        let params = params.unwrap();

        // Keep the first npars-1 params, then add one for each ... argument.
        let mut vars: Vec<crate::ObjectId> = Vec::with_capacity(nargs);
        for i in 0..npars - 1 {
            vars.push(tuple_at(&self.types, params, i));
        }
        let last = tuple_at(&self.types, params, npars - 1);
        let last_typ = last
            .typ(&self.objects)
            .unwrap_or_else(|| self.invalid_type());
        let last_name = last.name(&self.objects).to_string();
        let elem = {
            let u = last_typ.underlying(&self.types);
            if matches!(self.types.get(u), TypeData::Slice(_)) {
                slice_elem(&self.types, u)
            } else {
                self.invalid_type()
            }
        };
        while vars.len() < nargs {
            let p = new_param(&mut self.objects, last_name.clone(), elem);
            p.set_pkg(&mut self.objects, self.pkg);
            vars.push(p);
        }
        new_tuple(&mut self.types, &vars)
    }

    /// The parameter type an argument at index `i` is assigned to, accounting
    /// for variadic spreading.
    fn param_type(
        &self,
        params: Option<TypeId>,
        npars: usize,
        variadic: bool,
        ddd: bool,
        i: usize,
    ) -> TypeId {
        let params = match params {
            Some(p) => p,
            None => return self.invalid_type(),
        };
        if !variadic || i < npars - 1 {
            let v = tuple_at(&self.types, params, i);
            return v.typ(&self.objects).unwrap_or_else(|| self.invalid_type());
        }
        // Variadic tail (i >= npars-1): the last parameter is a slice `[]E`.
        let last = tuple_at(&self.types, params, npars - 1);
        let last_typ = last
            .typ(&self.objects)
            .unwrap_or_else(|| self.invalid_type());
        if ddd {
            // `f(a, b, s...)` — the spread argument has the slice type itself.
            last_typ
        } else {
            // `f(a, b, c)` — each tail argument has the element type.
            let u = last_typ.underlying(&self.types);
            if matches!(self.types.get(u), TypeData::Slice(_)) {
                slice_elem(&self.types, u)
            } else {
                self.invalid_type()
            }
        }
    }

    /// Evaluate each expression for its side effects (so variables are marked
    /// used and errors surface) discarding the result. Mirrors `Checker.use`.
    fn use_args(&mut self, args: &[Expr]) {
        for a in args {
            let mut op = Operand::invalid();
            self.expr(&mut op, a);
        }
    }
    /// Resolve a qualified identifier `pkg.sel` where `pkg` names an imported
    /// package. Looks `sel` up in the package's scope and fills `x` from the
    /// resolved object (which is always fully initialized — imported objects
    /// don't need `objDecl`).
    ///
    /// Extracted from the front of `Checker.selector`. The cgo special cases
    /// and `usedPkgNames` bookkeeping are omitted.
    fn qualified_ident<'a>(
        &mut self,
        x: &mut Operand<'a>,
        sel_expr: &'a Expr,
        e: &SelectorExpr,
        pkg: crate::PackageId,
        sel: &str,
    ) {
        let pkg_scope = self.packages.get(pkg).scope();
        let exp = match scope_lookup(&self.scopes, pkg_scope, sel) {
            Some(o) => o,
            None => {
                let pname = self.packages.get(pkg).name().to_string();
                // Prefer a case-insensitive hint when an exported name matches (D03).
                let hint = lookup_ignoring_case(&self.scopes, pkg_scope, sel, true)
                    .into_iter()
                    .next()
                    .map(|o| o.name(&self.objects).to_string());
                let msg = match hint {
                    Some(alt) => format!("undefined: {}.{} (but have {})", pname, sel, alt),
                    None => format!("undefined: {}.{}", pname, sel),
                };
                self.error(e.sel.pos().0 as u32, Code::UndeclaredImportedName, msg);
                self.set_selector_error(x, sel_expr);
                return;
            }
        };

        if !exp.exported(&self.objects) {
            let pname = self.packages.get(pkg).name().to_string();
            self.error(
                e.sel.pos().0 as u32,
                Code::UnexportedName,
                format!("name {} not exported by package {}", sel, pname),
            );
            // ok to continue
        }

        // The selector identifier denotes the resolved imported object.
        self.record_use(&e.sel, exp);

        let typ = exp
            .typ(&self.objects)
            .unwrap_or_else(|| self.invalid_type());
        match self.objects.get(exp) {
            ObjectData::Const(c) => {
                x.mode = OperandMode::Constant;
                x.val = Some(c.val().clone());
            }
            ObjectData::TypeName(_) => x.mode = OperandMode::TypeExpr,
            ObjectData::Var(_) => x.mode = OperandMode::Variable,
            ObjectData::Func(_) => x.mode = OperandMode::Value,
            ObjectData::Builtin(b) => {
                x.id = Some(b.id());
                x.mode = OperandMode::Builtin;
            }
            _ => {
                // PkgName/Nil can't be looked up in another package's scope.
                self.set_selector_error(x, sel_expr);
                return;
            }
        }
        x.typ = Some(typ);
        x.expr = Some(sel_expr);
    }

    /// Resolve a selector expression `x.f`, recording the result in `x`.
    ///
    /// Equivalent to `Checker.selector`. `want_type` requests that the whole
    /// selector denote a type (it never can here, so it is an error — matching
    /// Go's go.dev/issue/57522 guard).
    ///
    /// A qualified identifier (`pkg.X`) is handled up front via
    /// [`Checker::qualified_ident`].
    pub fn selector<'a>(&mut self, x: &mut Operand<'a>, sel_expr: &'a Expr, want_type: bool) {
        let e = match sel_expr {
            Expr::SelectorExpr(s) => s,
            _ => panic!("selector: expected SelectorExpr"),
        };
        let sel = e.sel.name.as_str().to_string();

        // Qualified identifier (`pkg.X`): if `e.X` is an identifier that resolves
        // to a package name, everything is handled here so operands never need a
        // dedicated "package" mode. With no Importer the only such package is
        // `unsafe`, whose scope holds the unsafe built-ins and `unsafe.Pointer`.
        if let Expr::Ident(ident) = e.x.as_ref() {
            if let Some(pname) = self.lookup(&ident.name) {
                if let Some(pkg) = pname.imported_pkg(&self.objects) {
                    // The leading identifier is a use of the package name.
                    self.record_use(ident, pname);
                    // Mark the package as used (Go: `usedPkgNames`), so the
                    // unused-import check does not flag it.
                    self.used_pkg_names.insert(pname);
                    self.qualified_ident(x, sel_expr, e, pkg, &sel);
                    return;
                }
            }
        }

        // Check the operand expression `e.X` (which may itself be a type, for
        // a method expression `T.M`).
        self.expr(x, &e.x);
        match x.mode {
            OperandMode::Builtin => {
                let xs = self.operand_str(x);
                self.error(
                    e.x.pos().0 as u32,
                    Code::UncalledBuiltin,
                    format!("invalid use of {} in selector expression", xs),
                );
                self.set_selector_error(x, sel_expr);
                return;
            }
            OperandMode::Invalid => {
                self.set_selector_error(x, sel_expr);
                return;
            }
            _ => {}
        }

        // A selector never denotes a type (go.dev/issue/57522).
        if want_type {
            self.error(
                e.sel.pos().0 as u32,
                Code::NotAType,
                format!("{}.{} is not a type", self.operand_str(x), sel),
            );
            self.set_selector_error(x, sel_expr);
            return;
        }

        let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
        let addressable = x.mode == OperandMode::Variable;
        let result = lookup_field_or_method(
            &mut self.types,
            &self.objects,
            &self.packages,
            xtyp,
            addressable,
            Some(self.pkg),
            &sel,
        );

        let (obj, index, indirect) = match result {
            LookupResult::Found {
                obj,
                index,
                indirect,
            } => (obj, index, indirect),
            LookupResult::Ambiguous { .. } => {
                self.error(
                    e.sel.pos().0 as u32,
                    Code::AmbiguousSelector,
                    format!("ambiguous selector {}.{}", self.operand_str(x), sel),
                );
                self.set_selector_error(x, sel_expr);
                return;
            }
            LookupResult::PtrRecvRequired => {
                let ts = self.type_str(xtyp);
                let msg = if x.mode == OperandMode::TypeExpr {
                    format!(
                        "invalid method expression {}.{} (needs pointer receiver (*{}).{})",
                        ts, sel, ts, sel
                    )
                } else {
                    format!("cannot call pointer method {} on {}", sel, ts)
                };
                self.error(e.sel.pos().0 as u32, Code::InvalidMethodExpr, msg);
                self.set_selector_error(x, sel_expr);
                return;
            }
            LookupResult::NotFound => {
                // Don't report another error if the underlying type was
                // invalid (go.dev/issue/49541).
                let under = xtyp.underlying(&self.types);
                if !is_valid(&self.types, under) {
                    self.set_selector_error(x, sel_expr);
                    return;
                }
                let why = if is_interface_ptr(&self.types, xtyp) {
                    "type is pointer to interface, not interface".to_string()
                } else {
                    format!(
                        "type {} has no field or method {}",
                        self.type_str(xtyp),
                        sel
                    )
                };
                self.error(
                    e.sel.pos().0 as u32,
                    Code::MissingFieldOrMethod,
                    format!("{}.{} undefined ({})", self.operand_str(x), sel, why),
                );
                self.set_selector_error(x, sel_expr);
                return;
            }
        };

        // Methods may not have a fully set-up signature yet.
        let is_func = matches!(self.objects.get(obj), ObjectData::Func(_));
        if is_func {
            self.obj_decl(obj);
        }

        if x.mode == OperandMode::TypeExpr {
            // Method expression `T.M`: the receiver becomes the first param.
            if !is_func {
                self.error(
                    e.sel.pos().0 as u32,
                    Code::MissingFieldOrMethod,
                    format!(
                        "{}.{} undefined (type {} has no method {})",
                        self.operand_str(x),
                        sel,
                        self.type_str(xtyp),
                        sel
                    ),
                );
                self.set_selector_error(x, sel_expr);
                return;
            }

            // recordSelection(MethodExpr) — also records the use of e.sel.
            self.record_selection(
                e,
                SelectionKind::MethodExpr,
                xtyp,
                obj,
                index.clone(),
                indirect,
            );

            let sig_id = self.method_sig_for_recv(xtyp, &index, obj);
            let (recv, params, results, variadic) = match self.types.get(sig_id) {
                TypeData::Signature(sig) => {
                    (sig.recv(), sig.params(), sig.results(), sig.variadic())
                }
                _ => (None, None, None, false),
            };
            if recv.is_none() {
                self.error(
                    e.sel.pos().0 as u32,
                    Code::InvalidDeclCycle,
                    "illegal cycle in method declaration".to_string(),
                );
                self.set_selector_error(x, sel_expr);
                return;
            }

            // Promote the receiver type (`x.typ`) to the new first parameter.
            let recv_name = recv
                .map(|r| r.name(&self.objects).to_string())
                .unwrap_or_default();
            let arg0 = crate::object::var::new_param(&mut self.objects, recv_name, xtyp);
            let mut new_params: Vec<crate::ObjectId> = vec![arg0];
            if let Some(p_id) = params {
                let n = match self.types.get(p_id) {
                    TypeData::Tuple(t) => t.len(),
                    _ => 0,
                };
                for i in 0..n {
                    let var = match self.types.get(p_id) {
                        TypeData::Tuple(t) => t.at(i),
                        _ => unreachable!(),
                    };
                    new_params.push(var);
                }
            }
            let new_params_tup = crate::tuple::new_tuple(&mut self.types, &new_params);
            let new_sig = new_signature_type(
                &mut self.types,
                None,
                &[],
                &[],
                new_params_tup,
                results,
                variadic,
            );
            x.mode = OperandMode::Value;
            x.typ = Some(new_sig);
            self.add_decl_dep(obj);
        } else {
            // Regular selector. recordSelection (which also records the use of
            // e.sel) runs before the kind dispatch so it doesn't clash with the
            // immutable `self.objects` borrow the match scrutinee holds.
            // `lookup_field_or_method` only ever returns a field Var or a
            // method Func here, so `is_func` selects the selection kind.
            let kind = if is_func {
                SelectionKind::MethodVal
            } else {
                SelectionKind::FieldVal
            };
            self.record_selection(e, kind, xtyp, obj, index.clone(), indirect);
            // For a method value, compute the (possibly instantiated) signature
            // *before* the `self.objects.get(obj)` borrow below, since
            // `method_sig_for_recv` borrows `self` mutably.
            let method_sig = if is_func {
                Some(self.method_sig_for_recv(xtyp, &index, obj))
            } else {
                None
            };
            match self.objects.get(obj) {
                ObjectData::Var(_) => {
                    x.mode = if x.mode == OperandMode::Variable || indirect {
                        OperandMode::Variable
                    } else {
                        OperandMode::Value
                    };
                    x.typ = obj.typ(&self.objects);
                }
                ObjectData::Func(_) => {
                    // Method value: copy the signature with the receiver
                    // removed. For a generic instance receiver the signature is
                    // instantiated with the receiver's type arguments first.
                    let sig_id = method_sig.unwrap_or_else(|| self.invalid_type());
                    let (params, results, variadic) = match self.types.get(sig_id) {
                        TypeData::Signature(sig) => (sig.params(), sig.results(), sig.variadic()),
                        _ => (None, None, false),
                    };
                    let new_sig = new_signature_type(
                        &mut self.types,
                        None,
                        &[],
                        &[],
                        params,
                        results,
                        variadic,
                    );
                    x.mode = OperandMode::Value;
                    x.typ = Some(new_sig);
                    self.add_decl_dep(obj);
                }
                _ => unreachable!("lookup returned a non-field, non-method object"),
            }
        }

        x.expr = Some(sel_expr);
    }

    /// The effective signature of method `method` selected on `recv_type` via
    /// the embedded-field path `index`.
    ///
    /// For a generic *instance* receiver (`Box[int]`), the looked-up method is
    /// the origin's method (`Box.Get`), whose signature still mentions the
    /// origin's type parameters (`T`). This substitutes the instance's type
    /// arguments (`int`) for the method's receiver type parameters, so the
    /// selected method has the instantiated signature — Go's `expandMethod`
    /// (the lazy `Named.Method(i)` expansion) done on demand at selection time.
    ///
    /// `index` is the full lookup path (embedded-field steps followed by the
    /// method index); the method may be *promoted* from an embedded generic
    /// field, in which case the receiver instance is not `recv_type` itself but
    /// the embedded type reached by walking `index`. Non-instance receivers
    /// return the method's signature unchanged.
    fn method_sig_for_recv(
        &mut self,
        recv_type: TypeId,
        index: &[i32],
        method: crate::ObjectId,
    ) -> TypeId {
        let sig = method
            .typ(&self.objects)
            .unwrap_or_else(|| self.invalid_type());
        // Walk the embedded-field steps (all index elements but the last, which
        // is the method index) to reach the type that actually declares the
        // method — for a promoted method this is an embedded field's type.
        let recv_owner = self.walk_embedded_path(recv_type, index);
        // The method may be found through a pointer receiver; look through it.
        let (base, _) = crate::lookup::deref(&self.types, recv_owner);
        let named = match crate::lookup::as_named(&self.types, base) {
            Some(n) => n,
            None => return sig,
        };
        let targs: Vec<TypeId> = match crate::named::named_type_args(&self.types, named) {
            Some(list) => list.list().to_vec(),
            None => return sig, // not an instance — nothing to substitute
        };
        let rparams: Vec<TypeId> =
            match crate::signature::signature_recv_type_params(&self.types, sig) {
                Some(list) => list.list().to_vec(),
                None => return sig,
            };
        if rparams.is_empty() || rparams.len() != targs.len() {
            return sig;
        }
        let smap = crate::subst::make_subst_map(&rparams, &targs);
        crate::subst::subst(
            &mut self.types,
            &mut self.objects,
            &smap,
            None,
            &mut self.ctxt,
            sig,
        )
    }

    /// Follow the embedded-field steps of a lookup `index` (every element except
    /// the last, which is the field/method index within the final type) from
    /// `start`, dereferencing pointers, and return the type reached — i.e. the
    /// type on which the looked-up member is actually declared.
    fn walk_embedded_path(&self, start: TypeId, index: &[i32]) -> TypeId {
        let mut t = start;
        if index.len() <= 1 {
            return t;
        }
        for &fi in &index[..index.len() - 1] {
            let (base, _) = crate::lookup::deref(&self.types, t);
            let u = base.underlying(&self.types);
            t = match self.types.get(u) {
                TypeData::Struct(s) if (fi as usize) < s.num_fields() => {
                    let f = s.field(fi as usize);
                    match f.typ(&self.objects) {
                        Some(ft) => ft,
                        None => return t,
                    }
                }
                _ => return t,
            };
        }
        t
    }

    /// Set the operand to the canonical invalid-selector state.
    /// Mirrors the `Error:` label in `Checker.selector`.
    fn set_selector_error<'a>(&mut self, x: &mut Operand<'a>, sel_expr: &'a Expr) {
        x.mode = OperandMode::Invalid;
        x.typ = Some(self.invalid_type());
        x.expr = Some(sel_expr);
    }

    /// Short rendering of an operand for error messages. Uses the expression
    /// source where available, else the operand's type.
    pub(crate) fn operand_str(&self, x: &Operand<'_>) -> String {
        match x.expr {
            Some(Expr::Ident(id)) => id.name.to_string(),
            _ => match x.typ {
                Some(t) => self.type_str(t),
                None => "<expr>".to_string(),
            },
        }
    }
}

/// Reports whether `e` is a syntactic *type* form — one that can only ever
/// denote a type, never a value.
///
/// Used to decide how a call's function operand is evaluated: these forms have
/// no expression semantics, so they must go through `typ`. `Expr::FuncType` is
/// safe here because the parser produces `Expr::FuncLit` for `func() { ... }`;
/// a bare `func(...)` signature in expression position is only ever a
/// conversion target. `StarExpr` is deliberately excluded: `*T(x)` parses as
/// `*(T(x))`, so a pointer conversion always arrives parenthesised.
fn is_type_syntax(e: &Expr) -> bool {
    matches!(
        e,
        Expr::ParenExpr(_)
            | Expr::ArrayType(_)
            | Expr::StructType(_)
            | Expr::FuncType(_)
            | Expr::InterfaceType(_)
            | Expr::MapType(_)
            | Expr::ChanType(_)
    )
}
