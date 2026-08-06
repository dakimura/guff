//! Port of `typexpr.go` — turning an AST type expression into a `TypeId`.
//!
//! This is **chunk 21a**: the entry points (`typ`/`typ_internal`), identifier
//! resolution (`ident`/`type_ident`), and the "basic" composite cases that are
//! pure recursion — parentheses, pointer (`*T`), and slice (`[]T`). The
//! remaining cases (sized arrays, `map`, `chan`, `struct`, `interface`,
//! function types, and generic instantiation `T[...]`) land in chunk 21b.
//!
//! ## Deferrals (chunk 21a)
//!
//! - `objDecl` (forcing an unresolved object's type) — wired in chunk 23: a
//!   `TypeName` whose `typ` isn't set yet is forced via [`Checker::obj_decl`].
//! - `recordTypeAndValue` is a no-op (chunk 18b).
//! - `verifyVersionf` (go1.18 gate on `any`/`comparable`), dot-import marking,
//!   `usedVars`, the `isGeneric` "needs instantiation" check, and `validVarType`
//!   (constraint-interface rejection) are omitted.
//! - The non-basic type-expression cases return `Typ[Invalid]` silently until
//!   chunk 21b.

use guff::ast::{ChanDir as AstChanDir, Expr, Ident};
use guff::token::Token;
use guff_constant::{int64_val, make_from_literal, to_int, Kind};

use crate::arena::{ObjectData, ObjectId, TypeData, TypeId};
use crate::array::new_array;
use crate::chan::{new_chan, ChanDir};
use crate::check::Checker;
use crate::check_lookup::implements;
use crate::instantiate::instantiate;
use crate::map::new_map;
use crate::operand::{Operand, OperandMode};
use crate::pointer::new_pointer;
use crate::predicates::{is_generic, is_valid};
use crate::scope::lookup_chain;
use crate::slice::new_slice;
use crate::subst::{make_subst_map, subst};
use crate::typeparam::type_param_constraint;
use guff_types_errors::Code;

impl Checker {
    /// Type-check the type expression `e` and return its type, or
    /// `Typ[Invalid]`.
    ///
    /// Equivalent to `Checker.typ` (→ `declaredType(e, nil)` → `typInternal`).
    pub fn typ(&mut self, e: &Expr) -> TypeId {
        let t = self.typ_internal(e);
        // Go's `typInternal` records every type expression as a `typexpr`-mode
        // entry in `Info.Types` (chunk 50). Sub-expressions reached via further
        // `typ` calls are recorded by their own invocation.
        self.record_type_and_value(e, OperandMode::TypeExpr, t, None);
        t
    }

    /// Look up `name` starting from the current environment scope, falling
    /// back to the universe scope.
    ///
    /// Simplified `Checker.lookupScope` (we don't track the defining scope for
    /// dot-import bookkeeping).
    pub fn lookup(&self, name: &str) -> Option<ObjectId> {
        if let Some(scope) = self.env.scope {
            if let Some(o) = lookup_chain(&self.scopes, scope, name) {
                return Some(o);
            }
        }
        lookup_chain(&self.scopes, self.universe_scope, name)
    }

    /// The core type-expression dispatch.
    ///
    /// Equivalent to `Checker.typInternal` (the chunk-21a subset).
    fn typ_internal(&mut self, e: &Expr) -> TypeId {
        match e {
            Expr::BadExpr(_) => self.invalid_type(), // error reported before
            Expr::Ident(id) => self.type_ident(id),
            Expr::ParenExpr(p) => self.typ_internal(&p.x),

            // *T — pointer.
            Expr::StarExpr(s) => {
                let base = self.typ(&s.x);
                // If the base is invalid, *base isn't useful — return invalid
                // (mirrors Go's go.dev/issue/49005 handling).
                if !is_valid(&self.types, base) {
                    return self.invalid_type();
                }
                new_pointer(&mut self.types, base)
            }

            // [N]T (array), []T (slice), or invalid [...]T outside a composite lit.
            Expr::ArrayType(a) => {
                let elem = self.typ(&a.elt);
                if crate::util::is_ddd_array(a) {
                    self.error(
                        a.lbrack.0 as u32,
                        Code::InvalidArrayLen,
                        "invalid use of [...] array (outside a composite literal)",
                    );
                    self.invalid_type()
                } else {
                    match &a.len {
                        None => new_slice(&mut self.types, elem),
                        Some(len) => match self.array_length(len) {
                            Some(n) => new_array(&mut self.types, elem, n),
                            // length was non-constant / not a literal: deferred or
                            // already reported.
                            None => self.invalid_type(),
                        },
                    }
                }
            }

            // map[K]V.
            Expr::MapType(m) => {
                let key = self.typ(&m.key);
                let elem = self.typ(&m.value);
                // DEFERRED (chunk 25): the `later` comparable-key-type check.
                new_map(&mut self.types, key, elem)
            }

            // chan T / chan<- T / <-chan T.
            Expr::ChanType(ch) => {
                let elem = self.typ(&ch.value);
                let dir = match ch.dir {
                    d if d == AstChanDir::SEND => ChanDir::SendOnly,
                    d if d == AstChanDir::RECV => ChanDir::RecvOnly,
                    // SEND|RECV (bidirectional), or any other value.
                    _ => ChanDir::SendRecv,
                };
                new_chan(&mut self.types, dir, elem)
            }

            // struct { ... } (chunk 33a).
            Expr::StructType(s) => self.struct_type(s),

            // func(...) ... — build via func_type (no receiver/type params).
            Expr::FuncType(ft) => self.func_type(None, ft),

            // interface { ... } (chunk 33b).
            Expr::InterfaceType(i) => self.interface_type(i),

            // T[A] — single-argument generic instantiation (chunk 35a).
            Expr::IndexExpr(ix) => {
                let args = std::slice::from_ref(ix.index.as_ref());
                self.instantiated_type(&ix.x, args, ix.x.pos().0 as u32)
            }
            // T[A, B, ...] — multi-argument generic instantiation (chunk 35a).
            Expr::IndexListExpr(ix) => {
                self.instantiated_type(&ix.x, &ix.indices, ix.x.pos().0 as u32)
            }

            // Package-qualified type name (`pkg.T`, e.g. `unsafe.Pointer`).
            // Mirrors Go's `typInternal` SelectorExpr case: run the selector in
            // type context and accept it only if it denotes a type.
            Expr::SelectorExpr(_) => {
                let mut x = Operand::invalid();
                self.selector(&mut x, e, true);
                match x.mode {
                    OperandMode::TypeExpr => x.typ.unwrap_or_else(|| self.invalid_type()),
                    // Invalid: error already reported by `selector`.
                    OperandMode::Invalid => self.invalid_type(),
                    OperandMode::NoValue => {
                        let xs = self.operand_str(&x);
                        self.error(
                            e.pos().0 as u32,
                            Code::NotAType,
                            format!("{} used as type", xs),
                        );
                        self.invalid_type()
                    }
                    _ => {
                        let xs = self.operand_str(&x);
                        self.error(
                            e.pos().0 as u32,
                            Code::NotAType,
                            format!("{} is not a type", xs),
                        );
                        self.invalid_type()
                    }
                }
            }

            _ => self.invalid_type(),
        }
    }

    /// Evaluate an array-length expression to a non-negative `i64`.
    ///
    /// Integer literals are folded directly; any other form is type-checked as
    /// a constant expression, so `const n = 20; type t [n]byte` works. A
    /// non-constant, non-integer, negative or unrepresentable length reports
    /// `InvalidArrayLen` and yields `None` (an invalid array type).
    ///
    /// Equivalent to `Checker.arrayLength`.
    fn array_length<'a>(&mut self, e: &'a Expr) -> Option<i64> {
        match e {
            Expr::BasicLit(lit) if lit.kind == Some(Token::INT) => {
                let val = to_int(make_from_literal(&lit.value, Token::INT, 0));
                if val.kind() == Kind::Int {
                    if let (n, true) = int64_val(&val) {
                        if n >= 0 {
                            return Some(n);
                        }
                    }
                }
                self.error(
                    e.pos().0 as u32,
                    Code::InvalidArrayLen,
                    format!("invalid array length {}", lit.value),
                );
                None
            }
            _ => {
                // An identifier length must name a constant. Go checks this up
                // front so that `type T [P]int` (a parameterized declaration
                // with a missing constraint) is reported as a bad array length
                // rather than as a mysterious non-constant expression.
                if let Expr::Ident(name) = e {
                    match self.lookup(name.name.as_str()) {
                        None => {
                            self.error(
                                e.pos().0 as u32,
                                Code::UndeclaredName,
                                format!("undefined: {}", name.name),
                            );
                            return None;
                        }
                        Some(obj) => {
                            if !matches!(self.objects.get(obj), ObjectData::Const(_)) {
                                self.error(
                                    e.pos().0 as u32,
                                    Code::InvalidArrayLen,
                                    format!("invalid array length {}", name.name),
                                );
                                return None;
                            }
                        }
                    }
                }

                let mut x = Operand::invalid();
                self.expr(&mut x, e);
                if x.mode != OperandMode::Constant {
                    if x.mode != OperandMode::Invalid {
                        let xs = self.operand_str(&x);
                        self.error(
                            e.pos().0 as u32,
                            Code::InvalidArrayLen,
                            format!("array length {} must be constant", xs),
                        );
                    }
                    return None;
                }

                let typ = x.typ.unwrap_or_else(|| self.invalid_type());
                let is_int = crate::predicates::is_integer(&self.types, typ);
                if crate::predicates::is_untyped(&self.types, typ) || is_int {
                    if let Some(v) = &x.val {
                        let val = to_int(v.clone());
                        if val.kind() == Kind::Int {
                            let int_t = self.basic(crate::basic::BasicKind::Int);
                            if crate::check_expr_const::representable_const(
                                &self.types,
                                &val,
                                int_t,
                            )
                            .is_some()
                            {
                                if let (n, true) = int64_val(&val) {
                                    if n >= 0 {
                                        return Some(n);
                                    }
                                }
                            }
                        }
                    }
                }

                let xs = self.operand_str(&x);
                let msg = if is_int {
                    format!("invalid array length {}", xs)
                } else {
                    format!("array length {} must be integer", xs)
                };
                self.error(e.pos().0 as u32, Code::InvalidArrayLen, msg);
                None
            }
        }
    }

    /// Resolve a type identifier to its `TypeId`.
    ///
    /// Simplified `Checker.ident` with `wantType = true`: looks the name up,
    /// requires it to denote a `TypeName`, and returns the type it names.
    fn type_ident(&mut self, id: &Ident) -> TypeId {
        let name = id.name.as_str();
        let obj = match self.lookup(name) {
            Some(o) => o,
            None => {
                let pos = id.pos().0 as u32;
                if name == "_" {
                    self.error(pos, Code::InvalidBlank, "cannot use _ as value or type");
                } else {
                    self.error(pos, Code::UndeclaredName, format!("undefined: {}", name));
                }
                return self.invalid_type();
            }
        };

        self.record_use(id, obj);
        self.mark_dot_import_use(obj);

        // Classify the object without holding an arena borrow across the
        // (mutating) objDecl call below.
        if !matches!(self.objects.get(obj), ObjectData::TypeName(_)) {
            let pos = id.pos().0 as u32;
            self.error(pos, Code::NotAType, format!("{} is not a type", name));
            return self.invalid_type();
        }

        // The type name hasn't been checked yet — force it now (chunk 23 wires
        // objDecl; recovers the chunk-21 deferral).
        if obj.typ(&self.objects).is_none() {
            self.obj_decl(obj);
        }
        match obj.typ(&self.objects) {
            Some(t) => t,
            None => self.invalid_type(),
        }
    }

    /// Type-check `x` as a type expression that must denote a *generic* type.
    ///
    /// Equivalent to `Checker.genericType` (with the `cause` out-param folded
    /// into a direct error report). If `x` resolves to a valid but
    /// non-generic type, reports `NotAGenericType` and returns `Typ[Invalid]`.
    pub(crate) fn generic_type(&mut self, x: &Expr) -> TypeId {
        let typ = self.typ_internal(x);
        if is_valid(&self.types, typ) && !is_generic(&self.types, typ) {
            self.error(
                x.pos().0 as u32,
                Code::NotAGenericType,
                format!("{} is not a generic type", self.type_str(typ)),
            );
            return self.invalid_type();
        }
        // DEFERRED: recordTypeAndValue(x, typexpr, typ) (chunk 37).
        typ
    }

    /// Evaluate a list of type-argument expressions.
    ///
    /// Equivalent to `Checker.typeList`: returns `None` if *any* argument is
    /// invalid (mirroring Go's `res = nil`), so the caller can bail out.
    pub(crate) fn type_list(&mut self, list: &[Expr]) -> Option<Vec<TypeId>> {
        let mut res = Vec::with_capacity(list.len());
        let mut ok = true;
        for x in list {
            let t = self.typ(x);
            if !is_valid(&self.types, t) {
                ok = false;
            }
            res.push(t);
        }
        if ok {
            Some(res)
        } else {
            None
        }
    }

    /// Check that the number of type arguments (`got`) matches the number of
    /// type parameters (`want`); report `WrongTypeArgCount` if not.
    ///
    /// Equivalent to `Checker.validateTArgLen` (the `check != nil` path).
    fn validate_targ_len(&mut self, pos: u32, name: &str, want: usize, got: usize) -> bool {
        let qual = if got < want {
            "not enough"
        } else if got > want {
            "too many"
        } else {
            return true;
        };
        self.error(
            pos,
            Code::WrongTypeArgCount,
            format!(
                "{} type arguments for type {}: have {}, want {}",
                qual, name, got, want
            ),
        );
        false
    }

    /// Instantiate a generic type expression `x[xlist...]`.
    ///
    /// Equivalent to `Checker.instantiatedType`. Resolves `x` to a generic
    /// type, evaluates the type arguments, checks their count, and builds the
    /// instance via [`crate::instantiate::instantiate`].
    ///
    /// Constraint *satisfaction* (`Checker.verify`) is checked via
    /// [`Checker::verify_targs`] (chunk 35b). `recordInstance` populates
    /// `Info.Instances` (chunk 53); `mono.recordInstance` remains a no-op.
    fn instantiated_type(&mut self, x: &Expr, xlist: &[Expr], pos: u32) -> TypeId {
        let gtyp = self.generic_type(x);
        if !is_valid(&self.types, gtyp) {
            return gtyp; // error already reported
        }

        // Evaluate type arguments; bail if any are invalid.
        let targs = match self.type_list(xlist) {
            Some(t) => t,
            None => return self.invalid_type(),
        };

        // Determine the type parameters and the type's name for diagnostics.
        let (tparams, name) = match self.types.get(gtyp) {
            TypeData::Named(n) => (
                n.type_params()
                    .map(|tp| tp.list().to_vec())
                    .unwrap_or_default(),
                n.obj().name(&self.objects).to_string(),
            ),
            TypeData::Alias(a) => (
                a.type_params()
                    .map(|tp| tp.list().to_vec())
                    .unwrap_or_default(),
                a.obj().name(&self.objects).to_string(),
            ),
            // genericType guaranteed a generic Named/Alias above.
            _ => return self.invalid_type(),
        };

        // Guard the instantiate panic on a count mismatch.
        if !self.validate_targ_len(pos, &name, tparams.len(), targs.len()) {
            return self.invalid_type();
        }

        // Check type constraints. A violation is a soft error: we still build
        // and return the instance (matching Go's `softErrorf` + return inst).
        if let Some((i, cause)) = self.verify_targs(&tparams, &targs) {
            let at = xlist.get(i).map(|e| e.pos().0 as u32).unwrap_or(pos);
            self.error(at, Code::InvalidTypeArg, cause);
        } else {
            // Record the instantiation in the monomorphization flow graph
            // (Go does this in the deferred `verify` closure's success path).
            self.mono.record_instance(
                &self.types,
                &self.objects,
                &self.scopes,
                &self.packages,
                self.pkg,
                pos,
                &tparams,
                &targs,
                xlist,
            );
        }

        let inst = instantiate(
            &mut self.types,
            &mut self.objects,
            &mut self.ctxt,
            gtyp,
            targs.clone(),
        );
        // recordInstance: map the instantiated identifier (`x`) to its type
        // arguments and resulting type. Go does this inside the deferred
        // `later` closure; we record eagerly since the instance is already
        // built (the verify step above is the only part that needs delaying,
        // and it is done inline).
        self.record_instance(x, targs, inst);
        inst
    }

    /// Verify that each type argument satisfies its type parameter's
    /// constraint. Returns the index and cause of the first violation, or
    /// `None` if all arguments satisfy their bounds.
    ///
    /// Equivalent to `Checker.verify`: substitutes each bound with the full
    /// argument list (a bound may be parameterized by the same type
    /// parameters — go.dev/issue/...) before calling `implements(targ, bound,
    /// constraint=true)`.
    pub(crate) fn verify_targs(
        &mut self,
        tparams: &[TypeId],
        targs: &[TypeId],
    ) -> Option<(usize, String)> {
        if tparams.is_empty() {
            return None;
        }
        let smap = make_subst_map(tparams, targs);
        for (i, &tpar) in tparams.iter().enumerate() {
            let bound = match type_param_constraint(&self.types, tpar) {
                Some(b) => subst(
                    &mut self.types,
                    &mut self.objects,
                    &smap,
                    None,
                    &mut self.ctxt,
                    b,
                ),
                None => continue,
            };
            if let Err(cause) = implements(
                &mut self.types,
                &self.objects,
                &self.packages,
                targs[i],
                bound,
                true,
            ) {
                return Some((i, cause));
            }
        }
        None
    }
}
