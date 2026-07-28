//! Port of statement type-checking from `go/types/stmt.go`.
//!
//! **Chunk 30a-1**: the dispatch skeleton ([`Checker::stmt`] / [`Checker::stmt_list`]
//! / [`Checker::simple_stmt`]), the lexical-scope helpers
//! ([`Checker::open_scope`] / [`Checker::close_scope`]), the [`StmtContext`]
//! bitset, and the `ExprStmt` / `LabeledStmt` / `BadStmt` / `EmptyStmt` cases.
//! The assignment statements (`AssignStmt`, `DeclStmt`, `IncDecStmt`,
//! `SendStmt`) land in 30a-2; the control-flow statements in 30b–30d. All of
//! those leave a `// DEFERRED` marker in the dispatch for now.
//!
//! Like the rest of the Checker engine this mirrors the `go/types` (go/ast)
//! shape, not `types2`'s `syntax` shape — the AST crate is go/ast-shaped.
//!
//! ## Deferrals (chunk-30a-1, see §8 / D24)
//!
//! - `funcBody`, `usage`, `isTerminating`, `labels`/`hasLabel` tracking, and
//!   `multipleDefaults`/`caseValues`/`caseTypes` are not ported yet (they come
//!   with the control-flow statements and `check_files`' func-body wiring).
//! - `ExprStmt`, `go`, and `defer` classify their operand via `rawExpr`'s
//!   [`ExprKind`](crate::ExprKind) return value (chunk 88): a conversion or a
//!   discarded expression is flagged, a call/receive/statement-builtin is
//!   allowed.
//! - `recordScope` is a no-op (Info recording — §18b).

use crate::hash::HashMap;

use guff::ast::{BasicLit, BinaryExpr, BlockStmt, CallExpr, Expr, Stmt};
use guff::token::Token;
use guff_constant::{make_bool, Kind as ConstKind, Value};
use guff_types_errors::Code;

use crate::arena::{ObjectData, TypeData};
use crate::basic::{BasicKind, RUNE};
use crate::check::Checker;
use crate::object::builtin::ExprKind;
use crate::object::var::{new_var, VarKind};
use crate::operand::{Operand, OperandMode};
use crate::predicates::{
    comparable, has_nil, identical, is_boolean, is_integer, is_interface, is_numeric, is_string,
    is_type_param, is_valid,
};
use crate::scope::{insert as scope_insert, new_scope};
use crate::tuple::{tuple_at, tuple_len};
use crate::{ObjectId, ScopeId, TypeId};

/// A hashable key derived from a constant case value, mirroring Go's `goVal`.
///
/// gc (and `go/types`) only checks duplicate cases for integer, floating-point
/// and string values; `go_val` returns `None` for every other kind (and for
/// integers that fit neither `i64` nor `u64`), which disables the check for
/// that case — matching Go's "implementation restriction of other compilers".
#[derive(PartialEq, Eq, Hash)]
enum CaseKey {
    Int(i64),
    Uint(u64),
    Float(u64), // f64 bit pattern (f64 is not Eq/Hash directly)
    Str(String),
}

/// Equivalent to Go's `goVal(constant.Value) any`.
fn go_val(val: &Value) -> Option<CaseKey> {
    match val.kind() {
        ConstKind::Int => {
            let (x, ok) = guff_constant::int64_val(val);
            if ok {
                return Some(CaseKey::Int(x));
            }
            let (x, ok) = guff_constant::uint64_val(val);
            if ok {
                return Some(CaseKey::Uint(x));
            }
            None
        }
        ConstKind::Float => {
            let (x, ok) = guff_constant::float64_val(val);
            ok.then(|| CaseKey::Float(x.to_bits()))
        }
        ConstKind::String => Some(CaseKey::Str(guff_constant::string_val(val))),
        _ => None,
    }
}

/// A bitset describing which control-flow statements are permissible in the
/// current context, plus extra context for better diagnostics.
///
/// Equivalent to `go/types`' `stmtContext`.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct StmtContext(u32);

impl StmtContext {
    /// `break` is allowed here.
    pub const BREAK_OK: StmtContext = StmtContext(1 << 0);
    /// `continue` is allowed here.
    pub const CONTINUE_OK: StmtContext = StmtContext(1 << 1);
    /// `fallthrough` is allowed here.
    pub const FALLTHROUGH_OK: StmtContext = StmtContext(1 << 2);
    /// This is the final case clause of a switch.
    pub const FINAL_SWITCH_CASE: StmtContext = StmtContext(1 << 3);
    /// We're inside a type switch.
    pub const IN_TYPE_SWITCH: StmtContext = StmtContext(1 << 4);

    /// The empty context.
    pub const EMPTY: StmtContext = StmtContext(0);

    /// Does this context contain all the bits in `other`?
    #[inline]
    pub fn contains(self, other: StmtContext) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for StmtContext {
    type Output = StmtContext;
    #[inline]
    fn bitor(self, rhs: StmtContext) -> StmtContext {
        StmtContext(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for StmtContext {
    #[inline]
    fn bitor_assign(&mut self, rhs: StmtContext) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for StmtContext {
    type Output = StmtContext;
    #[inline]
    fn bitand(self, rhs: StmtContext) -> StmtContext {
        StmtContext(self.0 & rhs.0)
    }
}

impl std::ops::Not for StmtContext {
    type Output = StmtContext;
    #[inline]
    fn not(self) -> StmtContext {
        StmtContext(!self.0)
    }
}

/// Peel parentheses off an expression, like `go/ast.Unparen`.
pub(crate) fn unparen(mut e: &Expr) -> &Expr {
    while let Expr::ParenExpr(p) = e {
        e = &p.x;
    }
    e
}

/// Does `e` denote a receive operation `<-ch` (possibly parenthesized)?
fn is_recv(e: &Expr) -> bool {
    matches!(unparen(e), Expr::UnaryExpr(u) if u.op == Token::ARROW)
}

/// Map a compound-assignment token (`+=`, `-=`, …) to its binary operator
/// (`+`, `-`, …). Returns `None` for any other token.
///
/// Equivalent to `go/types.assignOp`.
fn assign_op(op: Token) -> Option<Token> {
    Some(match op {
        Token::AddAssign => Token::ADD,
        Token::SubAssign => Token::SUB,
        Token::MulAssign => Token::MUL,
        Token::QuoAssign => Token::QUO,
        Token::RemAssign => Token::REM,
        Token::AndAssign => Token::AND,
        Token::OrAssign => Token::OR,
        Token::XorAssign => Token::XOR,
        Token::ShlAssign => Token::SHL,
        Token::ShrAssign => Token::SHR,
        Token::AndNotAssign => Token::AndNot,
        _ => return None,
    })
}

impl Checker {
    /// Open a new lexical scope as a child of the current environment scope and
    /// make it current. Returns the new scope so the caller can record it in
    /// [`Info::scopes`](crate::Info) via [`record_scope`](Self::record_scope).
    ///
    /// Equivalent to `Checker.openScope` (minus the inline `recordScope` — our
    /// callers record explicitly because they hold the scope-opening node id).
    pub fn open_scope(&mut self, pos: u32, end: u32, comment: &str) -> ScopeId {
        let parent = self.env.scope;
        let scope = new_scope(
            &mut self.scopes,
            parent,
            Some(self.universe_scope),
            pos,
            end,
            comment,
        );
        self.env.scope = Some(scope);
        scope
    }

    /// Record that `node_id` (a scope-bearing statement / file node) opens
    /// `scope`, for `Info::scopes`. Synthetic nodes carry id `0` and are
    /// skipped, matching Go's behaviour of only recording source nodes.
    /// Equivalent to `Checker.recordScope`.
    pub fn record_scope(&mut self, node_id: u32, scope: ScopeId) {
        if node_id != 0 {
            self.info.scopes.insert(node_id, scope);
        }
    }

    /// Record that `node_id` implicitly declares `obj` (an object with no
    /// explicit identifier in the source), for `Info::implicits`. Synthetic
    /// nodes carry id `0` and are skipped. Equivalent to `Checker.recordImplicit`.
    pub fn record_implicit(&mut self, node_id: u32, obj: ObjectId) {
        if node_id != 0 {
            self.info.implicits.insert(node_id, obj);
        }
    }

    /// Restore the parent of the current scope as the current scope.
    ///
    /// Equivalent to `Checker.closeScope`.
    pub fn close_scope(&mut self) {
        if let Some(s) = self.env.scope {
            self.env.scope = self.scopes.get(s).parent();
        }
    }

    /// Type-check a `go`/`defer` call. The statement form requires a function
    /// call (not a conversion, builtin, or bare expression).
    ///
    /// Simplified `Checker.suspendedCall`: the call's arguments are checked via
    /// [`Checker::call_expr`], but the conversion / "discards result" /
    /// uncalled-builtin classification (which needs `rawExpr`'s `exprKind`) is
    /// DEFERRED.
    fn suspended_call(&mut self, keyword: &str, call: &CallExpr) {
        let mut x = Operand::invalid();
        let kind = self.call_expr(&mut x, call);
        // Go's `suspendedCall`: a `go`/`defer` target must be a plain call, not
        // a conversion (which produces a value) or an expression-builtin (whose
        // result is discarded).
        let (msg, code) = match kind {
            ExprKind::Conversion => {
                let code = if keyword == "go" {
                    Code::InvalidGo
                } else {
                    Code::InvalidDefer
                };
                ("requires function call, not conversion", code)
            }
            ExprKind::Expression => ("discards result of", Code::UnusedResults),
            ExprKind::Statement => return,
        };
        let xs = self.operand_str(&x);
        self.error(x.pos() as u32, code, format!("{} {} {}", keyword, msg, xs));
    }

    /// Type-check a block: open a fresh scope, check the statement list, then
    /// close the scope. Equivalent to `Checker.stmt`'s `*ast.BlockStmt` case.
    fn check_block(&mut self, ctxt: StmtContext, b: &BlockStmt) {
        let scope = self.open_scope(b.lbrace.0 as u32, b.end().0 as u32, "block");
        self.record_scope(b.id, scope);
        self.stmt_list(ctxt, &b.list);
        self.close_scope();
    }

    /// Does the underlying of `t` denote a boolean type? (`allBoolean` without
    /// the type-parameter term iteration, which is approximated by `Underlying`.)
    fn all_boolean(&self, t: Option<TypeId>) -> bool {
        match t {
            Some(t) => is_boolean(&self.types, t.underlying(&self.types)),
            None => false,
        }
    }

    /// The element type of `x` when used as a send destination (`x <- v`).
    /// Reports `InvalidSend` and returns `None` if `x` is not a sendable
    /// channel.
    ///
    /// Simplified `Checker.chanElem` for the send (`recv == false`) case: the
    /// type-parameter (`commonUnder` over a type set) path is DEFERRED — the
    /// underlying type is used directly.
    fn send_chan_elem(&mut self, x: &Operand, pos: u32) -> Option<TypeId> {
        let xtyp = x.typ?;
        let u = xtyp.underlying(&self.types);
        match self.types.get(u) {
            TypeData::Chan(_) => {
                if crate::chan::chan_dir(&self.types, u) == crate::chan::ChanDir::RecvOnly {
                    let xs = self.operand_str(x);
                    self.error(
                        pos,
                        Code::InvalidSend,
                        format!("cannot send to receive-only channel {}", xs),
                    );
                    None
                } else {
                    Some(crate::chan::chan_elem(&self.types, u))
                }
            }
            _ => {
                let xs = self.operand_str(x);
                self.error(
                    pos,
                    Code::InvalidSend,
                    format!("cannot send to non-channel {}", xs),
                );
                None
            }
        }
    }

    /// The key and value types produced by a `range` clause over an expression
    /// of type `orig`. Returns `(key, val, ok)`; `None` for an absent key/val.
    ///
    /// Simplified `rangeKeyVal`: `commonUnder` is approximated by `Underlying`
    /// (no type-set iteration), and the **function-iterator** form (`range over
    /// func`, go1.23) is DEFERRED — it reports `ok == false`. Integer ranges,
    /// strings, arrays, `*array`, slices, maps, and channels are handled.
    fn range_key_val(&self, orig: TypeId) -> (Option<TypeId>, Option<TypeId>, bool) {
        let u = orig.underlying(&self.types);
        // arrayPtrDeref: range over `*[N]T` ranges over the array.
        let t = if let TypeData::Pointer(_) = self.types.get(u) {
            let base = crate::pointer::pointer_elem(&self.types, u);
            let bu = base.underlying(&self.types);
            if matches!(self.types.get(bu), TypeData::Array(_)) {
                bu
            } else {
                u
            }
        } else {
            u
        };

        match self.types.get(t) {
            TypeData::Basic(_) => {
                if is_string(&self.types, t) {
                    (
                        Some(self.basic(BasicKind::Int)),
                        Some(self.basic(RUNE)),
                        true,
                    )
                } else if is_integer(&self.types, t) {
                    // range over integer (go1.22): key is the range type, no value.
                    (Some(orig), None, true)
                } else {
                    (None, None, false)
                }
            }
            TypeData::Array(_) => (
                Some(self.basic(BasicKind::Int)),
                Some(crate::array::array_elem(&self.types, t)),
                true,
            ),
            TypeData::Slice(_) => (
                Some(self.basic(BasicKind::Int)),
                Some(crate::slice::slice_elem(&self.types, t)),
                true,
            ),
            TypeData::Map(_) => (
                Some(crate::map::map_key(&self.types, t)),
                Some(crate::map::map_elem(&self.types, t)),
                true,
            ),
            TypeData::Chan(_) => {
                if crate::chan::chan_dir(&self.types, t) == crate::chan::ChanDir::SendOnly {
                    (None, None, false)
                } else {
                    (Some(crate::chan::chan_elem(&self.types, t)), None, true)
                }
            }
            // range-over-func (go1.23): `f` is `func(yield func(K[, V]) bool)`.
            // The key/value types are the yield callback's parameter types.
            TypeData::Signature(_) => {
                let params = crate::signature::signature_params(&self.types, t);
                let results = crate::signature::signature_results(&self.types, t);
                // The iterator: exactly one parameter (yield), no results.
                if crate::tuple::tuple_len(&self.types, params) != 1
                    || crate::tuple::tuple_len(&self.types, results) != 0
                {
                    return (None, None, false);
                }
                let yield_var = crate::tuple::tuple_at(&self.types, params.unwrap(), 0);
                let yield_typ = match yield_var.typ(&self.objects) {
                    Some(ty) => ty,
                    None => return (None, None, false),
                };
                // The yield argument must itself be a function.
                let cb = yield_typ.underlying(&self.types);
                if !matches!(self.types.get(cb), TypeData::Signature(_)) {
                    return (None, None, false);
                }
                let cb_params = crate::signature::signature_params(&self.types, cb);
                let cb_results = crate::signature::signature_results(&self.types, cb);
                let np = crate::tuple::tuple_len(&self.types, cb_params);
                // yield takes at most two params and returns exactly one `bool`
                // (a named boolean type is rejected — go.dev/issue/71131).
                if np > 2 || crate::tuple::tuple_len(&self.types, cb_results) != 1 {
                    return (None, None, false);
                }
                let res0 = crate::tuple::tuple_at(&self.types, cb_results.unwrap(), 0);
                let res0_t = res0.typ(&self.objects).unwrap_or_else(|| self.invalid_type());
                if res0_t != self.basic(BasicKind::Bool) {
                    return (None, None, false);
                }
                let param_typ = |i: usize| {
                    crate::tuple::tuple_at(&self.types, cb_params.unwrap(), i)
                        .typ(&self.objects)
                        .unwrap_or_else(|| self.invalid_type())
                };
                let key = if np >= 1 { Some(param_typ(0)) } else { None };
                let val = if np >= 2 { Some(param_typ(1)) } else { None };
                (key, val, true)
            }
            _ => (None, None, false),
        }
    }

    /// Type-check a function body against its signature `sig`.
    ///
    /// Opens a fresh function scope (child of `parent_scope`, the declaring
    /// file scope), declares the receiver / parameters / named results into it,
    /// then checks the body statement list with `env.sig` set so `return`
    /// statements can be validated.
    ///
    /// Equivalent to `Checker.funcBody`. **Deferred**: `isTerminating`
    /// (MissingReturn), `usage` (declared-and-not-used), label resolution, and
    /// `Trace`. The signature carries no scope (D20), so parameters are declared
    /// into a fresh scope here rather than reusing `sig.scope`.
    ///
    /// `func_type_id` is the node id of the `FuncType` (from the `FuncDecl` or
    /// `FuncLit`); the fresh function scope is recorded under it in
    /// `Info::scopes` (Go `recordScope(ftyp, check.scope)` in `funcType`).
    pub fn func_body(
        &mut self,
        decl: Option<ObjectId>,
        sig: TypeId,
        func_type_id: u32,
        parent_scope: Option<ScopeId>,
        body: &BlockStmt,
    ) {
        let saved_scope = self.env.scope;
        let saved_sig = self.env.sig.take();
        let saved_decl = self.env.decl;
        // Track the enclosing package-level declaration so identifiers in the
        // body add dependency edges (for init_order). Equivalent to Go's
        // `environment{decl: decl, ...}` in `funcBody`.
        self.env.decl = decl;

        let fscope = new_scope(
            &mut self.scopes,
            parent_scope,
            Some(self.universe_scope),
            body.pos().0 as u32,
            body.end().0 as u32,
            "function",
        );
        // Mark this as a function scope so `usage` doesn't descend into nested
        // function-literal scopes a second time (they run their own func_body).
        self.scopes.get_mut(fscope).set_is_func(true);
        // Record the function scope under its FuncType node (Go keys the
        // function scope on `*ast.FuncType`, not the body block).
        self.record_scope(func_type_id, fscope);

        // Declare the type parameters (so the body can reference them), then
        // the receiver, parameters, and named results.
        for tp in self.signature_type_param_objs(sig) {
            let name = tp.name(&self.objects).to_string();
            if name != "_" && !name.is_empty() {
                scope_insert(&mut self.scopes, &mut self.objects, fscope, tp);
            }
        }
        for v in self.signature_vars(sig) {
            let name = v.name(&self.objects).to_string();
            if name != "_" && !name.is_empty() {
                scope_insert(&mut self.scopes, &mut self.objects, fscope, v);
            }
        }

        self.env.scope = Some(fscope);
        self.env.sig = Some(sig);
        self.stmt_list(StmtContext::EMPTY, &body.list);
        self.env.scope = saved_scope;
        self.env.sig = saved_sig;
        self.env.decl = saved_decl;

        // Resolve labels (declared/used/placement) across the whole body.
        self.labels(body);

        // Report local variables that were declared but never used.
        self.usage(fscope);

        // A function with results must end in a terminating statement.
        let has_results = match self.types.get(sig) {
            TypeData::Signature(s) => tuple_len(&self.types, s.results()) > 0,
            _ => false,
        };
        if has_results && !crate::return_check::is_terminating_list(&body.list, "") {
            self.error(
                body.end().0 as u32,
                Code::MissingReturn,
                "missing return".to_string(),
            );
        }
    }

    /// Report "declared and not used" for local variables in `scope` and its
    /// nested non-function child scopes.
    ///
    /// Equivalent to `Checker.usage`. Receiver/parameter/result variables are
    /// exempt; function-literal scopes are skipped (they run their own
    /// `func_body` → `usage`).
    fn usage(&mut self, scope: ScopeId) {
        // Collect unused local variables declared directly in this scope.
        let names = self.scopes.get(scope).names();
        let mut unused: Vec<ObjectId> = Vec::new();
        for name in names {
            let obj = match self.scopes.get(scope).lookup_local(&name) {
                Some(o) => o,
                None => continue,
            };
            if let ObjectData::Var(v) = self.objects.get(obj) {
                let needs_use =
                    !matches!(v.kind(), VarKind::Recv | VarKind::Param | VarKind::Result);
                if needs_use && !self.used_vars.contains(&obj) {
                    unused.push(obj);
                }
            }
        }
        unused.sort_by_key(|o| o.pos(&self.objects));
        for v in unused {
            let pos = v.pos(&self.objects);
            let name = v.name(&self.objects).to_string();
            self.error(
                pos,
                Code::UnusedVar,
                format!("declared and not used: {}", name),
            );
        }

        // Recurse into non-function child scopes.
        let n = self.scopes.get(scope).num_children();
        for i in 0..n {
            let child = self.scopes.get(scope).child(i);
            if !self.scopes.get(child).is_func() {
                self.usage(child);
            }
        }
    }

    /// The receiver, parameter, and result variables of `sig` (in that order).
    fn signature_vars(&self, sig: TypeId) -> Vec<ObjectId> {
        let mut out = Vec::new();
        if let TypeData::Signature(s) = self.types.get(sig) {
            if let Some(r) = s.recv() {
                out.push(r);
            }
            for t in [s.params(), s.results()] {
                if let Some(tt) = t {
                    let n = tuple_len(&self.types, Some(tt));
                    for i in 0..n {
                        out.push(tuple_at(&self.types, tt, i));
                    }
                }
            }
        }
        out
    }

    /// The `TypeName` objects of the signature's type parameters (empty for a
    /// non-generic function).
    fn signature_type_param_objs(&self, sig: TypeId) -> Vec<ObjectId> {
        let mut out = Vec::new();
        if let TypeData::Signature(s) = self.types.get(sig) {
            if let Some(tps) = s.type_params() {
                for &tp in tps.list() {
                    out.push(crate::typeparam::type_param_obj(&self.types, tp));
                }
            }
        }
        out
    }

    /// The result variables of the signature whose body is being checked.
    /// Empty when not inside a function body or the function has no results.
    fn sig_results(&self) -> Vec<ObjectId> {
        let sig = match self.env.sig {
            Some(s) => s,
            None => return Vec::new(),
        };
        let results = match self.types.get(sig) {
            TypeData::Signature(s) => s.results(),
            _ => None,
        };
        let mut v = Vec::new();
        if let Some(rt) = results {
            let n = tuple_len(&self.types, Some(rt));
            for i in 0..n {
                v.push(tuple_at(&self.types, rt, i));
            }
        }
        v
    }

    /// Report a `DuplicateDefault` error for each `default` case beyond the
    /// first in a switch/select body.
    ///
    /// Equivalent to `Checker.multipleDefaults` (the `CommClause` arm lands
    /// with `select`, 30d).
    fn multiple_defaults(&mut self, list: &[Stmt]) {
        let mut seen = false;
        for s in list {
            let is_default = match s {
                Stmt::CaseClause(c) => c.list.is_empty(),
                _ => continue,
            };
            if is_default {
                if seen {
                    self.error(
                        s.pos().0 as u32,
                        Code::DuplicateDefault,
                        "multiple defaults in switch",
                    );
                } else {
                    seen = true;
                }
            }
        }
    }

    /// Type-check the case values of an expression-switch case clause against
    /// the switch tag operand `x`.
    ///
    /// Equivalent to `Checker.caseValues`. **Deferred**: duplicate-case-value
    /// detection (the `goVal`/`valueMap` bookkeeping).
    fn case_values(
        &mut self,
        x: &Operand,
        values: &[Expr],
        seen: &mut HashMap<CaseKey, Vec<(TypeId, u32)>>,
    ) {
        for e in values {
            let mut v = Operand::invalid();
            self.expr(&mut v, e);
            if x.mode == OperandMode::Invalid || v.mode == OperandMode::Invalid {
                continue;
            }
            let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
            self.convert_untyped(&mut v, xtyp);
            if v.mode == OperandMode::Invalid {
                continue;
            }
            // Compare v against x (error positions land at the case value).
            let mut res = v.clone();
            let mut xx = x.clone();
            self.comparison(&mut res, &mut xx, Token::EQL, e.pos().0 as u32);
            if res.mode == OperandMode::Invalid {
                continue;
            }
            if v.mode != OperandMode::Constant {
                continue; // non-constant case: nothing to deduplicate
            }
            // Look for duplicate values. gc (and Go) only flag duplicates for
            // integer, floating-point and string constants — `go_val` returns
            // `None` for everything else. Two values may share an underlying
            // value but differ in type (e.g. `byte(1)` vs `myByte(1)` under an
            // interface switch), so we also compare types via `Identical`.
            let key = match v.val.as_ref().and_then(go_val) {
                Some(k) => k,
                None => continue,
            };
            let vtyp = v.typ.unwrap_or_else(|| self.invalid_type());
            // Snapshot the previously-seen types for this key to avoid holding
            // a borrow of `seen` across the `Identical` calls on `self.types`.
            let prev: Vec<TypeId> = seen
                .get(&key)
                .map(|list| list.iter().map(|(t, _)| *t).collect())
                .unwrap_or_default();
            let dup = prev.iter().any(|&other| {
                identical(&mut self.types, &self.objects, &self.packages, vtyp, other)
            });
            if dup {
                let vs = self.operand_str(&v);
                self.error(
                    e.pos().0 as u32,
                    Code::DuplicateCase,
                    format!("duplicate case {} in expression switch", vs),
                );
                continue;
            }
            seen.entry(key).or_default().push((vtyp, e.pos().0 as u32));
        }
    }

    /// Reports whether `e` is the predeclared `nil` (possibly parenthesised).
    ///
    /// Equivalent to `Checker.isNil`.
    fn is_nil_expr(&self, e: &Expr) -> bool {
        let mut cur = e;
        while let Expr::ParenExpr(p) = cur {
            cur = &p.x;
        }
        if let Expr::Ident(id) = cur {
            if let Some(obj) = self.lookup(&id.name) {
                return matches!(self.objects.get(obj), ObjectData::Nil(_));
            }
        }
        false
    }

    /// Type-check the type expressions of a type-switch case clause, detect
    /// duplicate cases via `seen`, and verify each type against the switch
    /// operand `sx` (when valid). Returns the case-specific type for the
    /// implicitly declared variable (`switch v := x.(type)`).
    ///
    /// Equivalent to `Checker.caseTypes`. `None` in `seen`/return models Go's
    /// `nil` type (the `case nil` of a type switch).
    fn case_types(
        &mut self,
        sx: Option<&Operand>,
        types: &[Expr],
        seen: &mut Vec<(Option<TypeId>, u32)>,
    ) -> Option<TypeId> {
        let mut t: Option<TypeId> = None;
        'outer: for e in types {
            // The spec allows the value nil instead of a type.
            if self.is_nil_expr(e) {
                t = None;
                let mut dummy = Operand::invalid();
                self.expr(&mut dummy, e); // run through expr for the usual checks
            } else {
                let ty = self.typ(e);
                if !is_valid(&self.types, ty) {
                    continue 'outer;
                }
                t = Some(ty);
            }
            // Look for duplicate types (quadratic, but type switches are small).
            for (other, _pos) in seen.iter() {
                let dup = match (t, *other) {
                    (None, None) => true,
                    (Some(a), Some(b)) => {
                        identical(&mut self.types, &self.objects, &self.packages, a, b)
                    }
                    _ => false,
                };
                if dup {
                    let ts = match t {
                        Some(ty) => self.type_str(ty),
                        None => "nil".to_string(),
                    };
                    self.error(
                        e.pos().0 as u32,
                        Code::DuplicateCase,
                        format!("duplicate case {} in type switch", ts),
                    );
                    continue 'outer;
                }
            }
            seen.push((t, e.pos().0 as u32));
            if let Some(sx) = sx {
                if let Some(ty) = t {
                    self.type_assertion(e.pos().0 as u32, sx, ty, true);
                }
            }
        }

        // spec: with exactly one type, the variable has that type; otherwise
        // (multiple types, or predeclared nil) it has the type of x.
        if types.len() != 1 || t.is_none() {
            t = sx.and_then(|x| x.typ);
        }
        t
    }

    /// Type-check an optional simple statement (e.g. an `if`/`for` init).
    ///
    /// Equivalent to `Checker.simpleStmt`.
    pub fn simple_stmt(&mut self, s: Option<&Stmt>) {
        if let Some(s) = s {
            self.stmt(StmtContext::EMPTY, s);
        }
    }

    /// Type-check a list of statements, threading the fallthrough context to
    /// the final statement only.
    ///
    /// Equivalent to `Checker.stmtList`. The `trimTrailingEmptyStmts` step is
    /// included so `fallthrough` analysis treats trailing empty statements as
    /// invisible.
    pub fn stmt_list(&mut self, ctxt: StmtContext, list: &[Stmt]) {
        let ok = ctxt.contains(StmtContext::FALLTHROUGH_OK);
        let inner = ctxt & !StmtContext::FALLTHROUGH_OK;

        // Trailing empty statements are "invisible" to fallthrough analysis.
        let mut n = list.len();
        while n > 0 && matches!(list[n - 1], Stmt::EmptyStmt(_)) {
            n -= 1;
        }

        for (i, s) in list[..n].iter().enumerate() {
            let mut inner = inner;
            if ok && i + 1 == n {
                inner |= StmtContext::FALLTHROUGH_OK;
            }
            self.stmt(inner, s);
        }
    }

    /// Type-check statement `s`.
    ///
    /// Equivalent to `Checker.stmt`. Collected function literals are processed
    /// at the end (Go's `defer check.processDelayed(len(check.delayed))`).
    pub fn stmt(&mut self, ctxt: StmtContext, s: &Stmt) {
        let top = self.delayed.len();

        // Reset context for statements of inner blocks.
        let inner = ctxt
            & !(StmtContext::FALLTHROUGH_OK
                | StmtContext::FINAL_SWITCH_CASE
                | StmtContext::IN_TYPE_SWITCH);

        match s {
            // ignore
            Stmt::BadStmt(_) | Stmt::EmptyStmt(_) => {}

            Stmt::DeclStmt(d) => self.decl_stmt(&d.decl),

            Stmt::LabeledStmt(l) => {
                // DEFERRED (30d): hasLabel tracking + check.labels(body).
                self.stmt(ctxt, &l.stmt);
            }

            Stmt::ExprStmt(es) => {
                // spec: "With the exception of specific built-in functions,
                // function and method calls and receive operations can appear
                // in statement context. Such statements may be parenthesized."
                let mut x = Operand::invalid();
                let kind = self.expr(&mut x, &es.x);
                let diag = match x.mode {
                    OperandMode::Invalid => None,
                    OperandMode::Builtin => Some(("must be called", Code::UncalledBuiltin)),
                    OperandMode::TypeExpr => Some(("is not an expression", Code::NotAnExpr)),
                    // A call/receive statement (`kind == Statement`) is allowed;
                    // a conversion or a bare expression's result is not used.
                    _ if kind == ExprKind::Statement => None,
                    _ => Some(("is not used", Code::UnusedExpr)),
                };
                if let Some((msg, code)) = diag {
                    let xs = self.operand_str(&x);
                    self.error(x.pos() as u32, code, format!("{} {}", xs, msg));
                }
            }

            Stmt::AssignStmt(a) => match a.tok {
                Some(Token::ASSIGN) | Some(Token::DEFINE) => {
                    if a.lhs.is_empty() {
                        self.error(
                            a.tok_pos.0 as u32,
                            Code::InvalidSyntaxTree,
                            "missing lhs in assignment",
                        );
                    } else if a.tok == Some(Token::DEFINE) {
                        self.short_var_decl(&a.lhs, &a.rhs);
                    } else {
                        self.assign_vars(&a.lhs, &a.rhs);
                    }
                }
                Some(tok) => {
                    // Compound assignment (`+=`, `-=`, …): single-valued only.
                    if a.lhs.len() != 1 || a.rhs.len() != 1 {
                        self.error(
                            a.tok_pos.0 as u32,
                            Code::MultiValAssignOp,
                            format!(
                                "assignment operation {} requires single-valued expressions",
                                tok.as_str()
                            ),
                        );
                    } else if let Some(op) = assign_op(tok) {
                        // Synthesize `lhs <op> rhs` and evaluate it; the result
                        // operand is then assigned back to the lhs.
                        let synth = Expr::BinaryExpr(BinaryExpr {
                            x: Box::new(a.lhs[0].clone()),
                            op_pos: a.tok_pos,
                            op,
                            y: Box::new(a.rhs[0].clone()),
                            id: 0, // synthetic node — not recorded
                        });
                        let mut x = Operand::invalid();
                        self.expr(&mut x, &synth);
                        if x.mode != OperandMode::Invalid {
                            self.assign_var(&a.lhs[0], None, Some(x), "assignment");
                        }
                    } else {
                        self.error(
                            a.tok_pos.0 as u32,
                            Code::InvalidSyntaxTree,
                            "unknown assignment operation",
                        );
                    }
                }
                None => {}
            },

            Stmt::IncDecStmt(s) => {
                let op = match s.tok {
                    Token::INC => Token::ADD,
                    Token::DEC => Token::SUB,
                    _ => {
                        self.error(
                            s.tok_pos.0 as u32,
                            Code::InvalidSyntaxTree,
                            "unknown inc/dec operation",
                        );
                        self.process_delayed(top);
                        return;
                    }
                };

                let mut x = Operand::invalid();
                self.expr(&mut x, &s.x);
                if x.mode != OperandMode::Invalid {
                    let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
                    let u = xtyp.underlying(&self.types);
                    if !is_numeric(&self.types, u) {
                        let (xs, ts) = (self.operand_str(&x), self.type_str(xtyp));
                        self.error(
                            s.x.pos().0 as u32,
                            Code::NonNumericIncDec,
                            format!("{}{} (non-numeric type {})", xs, s.tok.as_str(), ts),
                        );
                    } else {
                        // Synthesize `x <op> 1` and assign it back to x.
                        let one_pos = s.x.pos();
                        let one = Expr::BasicLit(BasicLit {
                            value_pos: one_pos,
                            value_end: one_pos,
                            kind: Some(Token::INT),
                            value: "1".to_string(),
                            id: 0, // synthetic node — not recorded
                        });
                        let synth = Expr::BinaryExpr(BinaryExpr {
                            x: Box::new(s.x.clone()),
                            op_pos: s.tok_pos,
                            op,
                            y: Box::new(one),
                            id: 0, // synthetic node — not recorded
                        });
                        let mut r = Operand::invalid();
                        self.expr(&mut r, &synth);
                        if r.mode != OperandMode::Invalid {
                            self.assign_var(&s.x, None, Some(r), "assignment");
                        }
                    }
                }
            }

            Stmt::SendStmt(s) => {
                let mut ch = Operand::invalid();
                let mut val = Operand::invalid();
                self.expr(&mut ch, &s.chan_);
                self.expr(&mut val, &s.value);
                if ch.mode != OperandMode::Invalid && val.mode != OperandMode::Invalid {
                    if let Some(elem) = self.send_chan_elem(&ch, s.arrow.0 as u32) {
                        self.assignment(&mut val, Some(elem), "send");
                    }
                }
            }

            Stmt::BlockStmt(b) => self.check_block(inner, b),

            Stmt::IfStmt(s) => {
                let scope = self.open_scope(s.if_.0 as u32, s.body.end().0 as u32, "if");
                self.record_scope(s.id, scope);

                self.simple_stmt(s.init.as_deref());
                let mut x = Operand::invalid();
                self.expr(&mut x, &s.cond);
                if x.mode != OperandMode::Invalid && !self.all_boolean(x.typ) {
                    self.error(
                        s.cond.pos().0 as u32,
                        Code::InvalidCond,
                        "non-boolean condition in if statement",
                    );
                }
                self.check_block(inner, &s.body);
                // The else branch must be another if, a block, or absent.
                match s.else_.as_deref() {
                    None | Some(Stmt::BadStmt(_)) => {}
                    Some(e @ (Stmt::IfStmt(_) | Stmt::BlockStmt(_))) => self.stmt(inner, e),
                    Some(e) => self.error(
                        e.pos().0 as u32,
                        Code::InvalidSyntaxTree,
                        "invalid else branch in if statement",
                    ),
                }

                self.close_scope();
            }

            Stmt::ForStmt(s) => {
                let inner = inner | StmtContext::BREAK_OK | StmtContext::CONTINUE_OK;
                let scope = self.open_scope(s.for_.0 as u32, s.body.end().0 as u32, "for");
                self.record_scope(s.id, scope);

                self.simple_stmt(s.init.as_deref());
                if let Some(cond) = &s.cond {
                    let mut x = Operand::invalid();
                    self.expr(&mut x, cond);
                    if x.mode != OperandMode::Invalid && !self.all_boolean(x.typ) {
                        self.error(
                            cond.pos().0 as u32,
                            Code::InvalidCond,
                            "non-boolean condition in for statement",
                        );
                    }
                }
                self.simple_stmt(s.post.as_deref());
                // spec: "the post statement must not be a short variable
                // declaration."
                if let Some(post) = s.post.as_deref() {
                    if let Stmt::AssignStmt(a) = post {
                        if a.tok == Some(Token::DEFINE) {
                            self.error(
                                post.pos().0 as u32,
                                Code::InvalidPostDecl,
                                "cannot declare in post statement",
                            );
                        }
                    }
                }
                self.check_block(inner, &s.body);

                self.close_scope();
            }

            Stmt::SwitchStmt(s) => {
                let inner = inner | StmtContext::BREAK_OK;
                let scope = self.open_scope(s.switch.0 as u32, s.body.end().0 as u32, "switch");
                self.record_scope(s.id, scope);

                self.simple_stmt(s.init.as_deref());
                let mut x = Operand::invalid();
                if let Some(tag) = &s.tag {
                    self.expr(&mut x, tag);
                    // Checking assignment to an invisible temporary gets all the
                    // relevant checks.
                    self.assignment(&mut x, None, "switch expression");
                    if x.mode != OperandMode::Invalid {
                        let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
                        let cmp = comparable(&mut self.types, &self.objects, &self.packages, xtyp);
                        let nilable = has_nil(&self.types, xtyp);
                        if !cmp && !nilable {
                            let (xs, ts) = (self.operand_str(&x), self.type_str(xtyp));
                            self.error(
                                x.pos() as u32,
                                Code::InvalidExprSwitch,
                                format!("cannot switch on {} ({} is not comparable)", xs, ts),
                            );
                            x.mode = OperandMode::Invalid;
                        }
                    }
                } else {
                    // spec: "A missing switch expression is equivalent to the
                    // boolean value true."
                    x.mode = OperandMode::Constant;
                    x.typ = Some(self.basic(BasicKind::Bool));
                    x.val = Some(make_bool(true));
                }

                self.multiple_defaults(&s.body.list);

                // Maps an underlying constant value to the (type, pos) pairs of
                // every case it has appeared in, for duplicate-case detection
                // across the whole switch (Go's `valueMap seen`).
                let mut seen: HashMap<CaseKey, Vec<(TypeId, u32)>> = HashMap::default();

                let n = s.body.list.len();
                for (i, c) in s.body.list.iter().enumerate() {
                    let clause = match c {
                        Stmt::CaseClause(cc) => cc,
                        _ => {
                            self.error(
                                c.pos().0 as u32,
                                Code::InvalidSyntaxTree,
                                "incorrect expression switch case",
                            );
                            continue;
                        }
                    };
                    self.case_values(&x, &clause.list, &mut seen);
                    let scope =
                        self.open_scope(clause.case.0 as u32, clause.colon.0 as u32, "case");
                    self.record_scope(clause.id, scope);
                    let mut inner2 = inner;
                    if i + 1 < n {
                        inner2 |= StmtContext::FALLTHROUGH_OK;
                    } else {
                        inner2 |= StmtContext::FINAL_SWITCH_CASE;
                    }
                    self.stmt_list(inner2, &clause.body);
                    self.close_scope();
                }

                self.close_scope();
            }

            Stmt::ReturnStmt(s) => {
                let res = self.sig_results();
                let named = res
                    .first()
                    .map(|v| !v.name(&self.objects).is_empty())
                    .unwrap_or(false);
                if s.results.is_empty() && !res.is_empty() && named {
                    // Return with implicit results (named results); allowed.
                    // spec restriction: a bare `return` is disallowed if a
                    // result parameter's name is shadowed by a different
                    // entity in scope at the return (Go's OutOfScopeResult).
                    for &obj in &res {
                        let name = obj.name(&self.objects).to_string();
                        if name.is_empty() {
                            continue;
                        }
                        if let Some(alt) = self.lookup(&name) {
                            if alt != obj {
                                self.error(
                                    s.return_.0 as u32,
                                    Code::OutOfScopeResult,
                                    format!("result parameter {} not in scope at return", name),
                                );
                            }
                        }
                    }
                } else {
                    self.init_vars(&res, &s.results, true);
                }
            }

            Stmt::RangeStmt(s) => {
                let inner = inner | StmtContext::BREAK_OK | StmtContext::CONTINUE_OK;
                let is_def = s.tok == Some(Token::DEFINE);

                let mut x = Operand::invalid();
                self.expr(&mut x, &s.x);

                // Determine key/value types.
                let (key, val) = if x.mode != OperandMode::Invalid {
                    let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
                    let (k, v, ok) = self.range_key_val(xtyp);
                    if !ok {
                        let xs = self.operand_str(&x);
                        self.error(
                            x.pos() as u32,
                            Code::InvalidRangeExpr,
                            format!("cannot range over {}", xs),
                        );
                    } else if k.is_none() && s.key.is_some() {
                        let xs = self.operand_str(&x);
                        self.error(
                            s.key.as_ref().unwrap().pos().0 as u32,
                            Code::InvalidIterVar,
                            format!("range over {} permits no iteration variables", xs),
                        );
                    } else if v.is_none() && s.value.is_some() {
                        let xs = self.operand_str(&x);
                        self.error(
                            s.value.as_ref().unwrap().pos().0 as u32,
                            Code::InvalidIterVar,
                            format!("range over {} permits only one iteration variable", xs),
                        );
                    }
                    (k, v)
                } else {
                    (None, None)
                };

                // Open the for block scope now (after the range clause), so
                // `:=` iteration variables go in it (go.dev/issue/51437).
                let scope = self.open_scope(s.for_.0 as u32, s.body.end().0 as u32, "range");
                self.record_scope(s.id, scope);

                let range_over_int = x
                    .typ
                    .map(|t| is_integer(&self.types, t.underlying(&self.types)))
                    .unwrap_or(false);

                let lhs = [s.key.as_ref(), s.value.as_ref()];
                let rhs = [key, val];

                if is_def {
                    let mut vars: Vec<ObjectId> = Vec::new();
                    for i in 0..2 {
                        let lhs_e = match lhs[i] {
                            Some(e) => e,
                            None => continue,
                        };
                        let invalid = self.invalid_type();
                        let obj = if let Expr::Ident(id) = lhs_e {
                            let name = id.name.clone();
                            let o = new_var(&mut self.objects, name.clone(), invalid);
                            o.set_pkg(&mut self.objects, self.pkg);
                            o.set_pos(&mut self.objects, id.pos().0 as u32);
                            if let ObjectData::Var(v) = self.objects.get_mut(o) {
                                v.set_kind(VarKind::Local);
                            }
                            if name != "_" {
                                vars.push(o);
                            }
                            // Record the def (Go passes the ident to declare,
                            // which calls recordDef). Recorded for `_` too.
                            self.record_def(id, Some(o));
                            o
                        } else {
                            self.error(
                                lhs_e.pos().0 as u32,
                                Code::InvalidSyntaxTree,
                                "cannot declare in range clause",
                            );
                            new_var(&mut self.objects, "_", invalid)
                        };

                        match rhs[i] {
                            None => {
                                if let ObjectData::Var(v) = self.objects.get_mut(obj) {
                                    v.set_typ(invalid);
                                }
                            }
                            Some(t) if range_over_int && i == 0 => {
                                self.init_var(obj, &mut x, "range clause");
                                let _ = t;
                            }
                            Some(t) => {
                                let mut y = Operand::invalid();
                                y.mode = OperandMode::Value;
                                y.typ = Some(t);
                                self.init_var(obj, &mut y, "assignment");
                            }
                        }
                    }

                    if !vars.is_empty() {
                        let scope_pos = s.body.pos().0 as u32;
                        for o in vars {
                            self.declare(scope, o, scope_pos);
                        }
                    } else {
                        self.error(
                            s.tok_pos.0 as u32,
                            Code::NoNewVar,
                            "no new variables on left side of :=",
                        );
                    }
                } else if s.key.is_some() {
                    // ordinary assignment to existing variables
                    for i in 0..2 {
                        let lhs_e = match lhs[i] {
                            Some(e) => e,
                            None => continue,
                        };
                        let t = match rhs[i] {
                            Some(t) => t,
                            None => continue,
                        };
                        if range_over_int && i == 0 {
                            self.assign_var(lhs_e, None, Some(x.clone()), "range clause");
                        } else {
                            let mut y = Operand::invalid();
                            y.mode = OperandMode::Value;
                            y.typ = Some(t);
                            self.assign_var(lhs_e, None, Some(y), "assignment");
                        }
                    }
                } else if range_over_int {
                    // No iteration variables, but still validate an integer
                    // range expression (`_ = x`).
                    self.assignment(&mut x, None, "range clause");
                }

                self.check_block(inner, &s.body);
                self.close_scope();
            }

            Stmt::GoStmt(s) => self.suspended_call("go", &s.call),

            Stmt::DeferStmt(s) => self.suspended_call("defer", &s.call),

            Stmt::BranchStmt(s) => {
                if s.label.is_some() {
                    // DEFERRED (labels.go 2nd pass): labelled break/continue/goto
                    // are resolved separately; nothing to check inline.
                } else {
                    match s.tok {
                        Token::BREAK => {
                            if !ctxt.contains(StmtContext::BREAK_OK) {
                                self.error(
                                    s.tok_pos.0 as u32,
                                    Code::MisplacedBreak,
                                    "break not in for, switch, or select statement",
                                );
                            }
                        }
                        Token::CONTINUE => {
                            if !ctxt.contains(StmtContext::CONTINUE_OK) {
                                self.error(
                                    s.tok_pos.0 as u32,
                                    Code::MisplacedContinue,
                                    "continue not in for statement",
                                );
                            }
                        }
                        Token::FALLTHROUGH => {
                            if !ctxt.contains(StmtContext::FALLTHROUGH_OK) {
                                let msg = if ctxt.contains(StmtContext::FINAL_SWITCH_CASE) {
                                    "cannot fallthrough final case in switch"
                                } else if ctxt.contains(StmtContext::IN_TYPE_SWITCH) {
                                    "cannot fallthrough in type switch"
                                } else {
                                    "fallthrough statement out of place"
                                };
                                self.error(s.tok_pos.0 as u32, Code::MisplacedFallthrough, msg);
                            }
                        }
                        _ => self.error(
                            s.tok_pos.0 as u32,
                            Code::InvalidSyntaxTree,
                            "branch statement",
                        ),
                    }
                }
            }

            Stmt::SelectStmt(s) => {
                let inner = inner | StmtContext::BREAK_OK;
                self.multiple_defaults(&s.body.list);

                for c in &s.body.list {
                    let clause = match c {
                        Stmt::CommClause(cc) => cc,
                        _ => continue, // error reported before
                    };

                    // clause.comm must be a SendStmt, RecvStmt, or default case.
                    let valid = match clause.comm.as_deref() {
                        None | Some(Stmt::SendStmt(_)) => true,
                        Some(Stmt::AssignStmt(a)) => a.rhs.len() == 1 && is_recv(&a.rhs[0]),
                        Some(Stmt::ExprStmt(e)) => is_recv(&e.x),
                        _ => false,
                    };
                    if !valid {
                        let pos = clause
                            .comm
                            .as_deref()
                            .map(|s| s.pos().0 as u32)
                            .unwrap_or(clause.case.0 as u32);
                        self.error(
                            pos,
                            Code::InvalidSelectCase,
                            "select case must be send or receive (possibly with assignment)",
                        );
                        continue;
                    }

                    let scope =
                        self.open_scope(clause.case.0 as u32, clause.colon.0 as u32, "case");
                    self.record_scope(clause.id, scope);
                    if let Some(comm) = clause.comm.as_deref() {
                        self.stmt(inner, comm);
                    }
                    self.stmt_list(inner, &clause.body);
                    self.close_scope();
                }
            }

            Stmt::TypeSwitchStmt(s) => {
                let inner = inner | StmtContext::BREAK_OK | StmtContext::IN_TYPE_SWITCH;
                let scope =
                    self.open_scope(s.switch.0 as u32, s.body.end().0 as u32, "type switch");
                self.record_scope(s.id, scope);

                self.simple_stmt(s.init.as_deref());

                // The guard has the form `[ ident ":=" ] x.(type)`, parsed into
                // either an ExprStmt (no lhs) or an AssignStmt (with lhs).
                let (mut lhs_name, guard_x): (Option<String>, Option<&Expr>) =
                    match s.assign.as_ref() {
                        Stmt::ExprStmt(e) => match &e.x {
                            Expr::TypeAssertExpr(ta) => (None, Some(&*ta.x)),
                            _ => (None, None),
                        },
                        Stmt::AssignStmt(a) => {
                            let name = match a.lhs.first() {
                                Some(Expr::Ident(id)) => Some(id.name.clone()),
                                _ => None,
                            };
                            let gx = match a.rhs.first() {
                                Some(Expr::TypeAssertExpr(ta)) => Some(&*ta.x),
                                _ => None,
                            };
                            (name, gx)
                        }
                        _ => (None, None),
                    };

                // `_ := x.(type)` is an invalid short variable declaration.
                if lhs_name.as_deref() == Some("_") {
                    self.error(
                        s.assign.pos().0 as u32,
                        Code::NoNewVar,
                        "no new variable on left side of :=",
                    );
                    lhs_name = None;
                }

                // Check the guard's right-hand side: it must be an interface.
                let mut sx: Option<Operand> = None;
                if let Some(gx) = guard_x {
                    let mut x = Operand::invalid();
                    self.expr(&mut x, gx);
                    if x.mode != OperandMode::Invalid {
                        let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
                        if is_type_param(&self.types, xtyp) {
                            let xs = self.operand_str(&x);
                            self.error(
                                x.pos() as u32,
                                Code::InvalidTypeSwitch,
                                format!("cannot use type switch on type parameter value {}", xs),
                            );
                        } else if is_interface(&self.types, xtyp) {
                            sx = Some(x);
                        } else {
                            let xs = self.operand_str(&x);
                            self.error(
                                x.pos() as u32,
                                Code::InvalidTypeSwitch,
                                format!("{} is not an interface", xs),
                            );
                        }
                    }
                }

                self.multiple_defaults(&s.body.list);

                let mut seen: Vec<(Option<TypeId>, u32)> = Vec::new();
                let mut clause_binding_objs: Vec<crate::arena::ObjectId> = Vec::new();
                for c in &s.body.list {
                    let clause = match c {
                        Stmt::CaseClause(cc) => cc,
                        _ => {
                            self.error(
                                c.pos().0 as u32,
                                Code::InvalidSyntaxTree,
                                "incorrect type switch case",
                            );
                            continue;
                        }
                    };
                    let case_t = self.case_types(sx.as_ref(), &clause.list, &mut seen);
                    let case_scope =
                        self.open_scope(clause.case.0 as u32, clause.colon.0 as u32, "case");
                    self.record_scope(clause.id, case_scope);
                    // Declare the case-local variable for `switch v := x.(type)`.
                    if let Some(name) = &lhs_name {
                        // The narrowed variable's declaration position is the
                        // guard's binding identifier (`v` in `v := x.(type)`).
                        let lhs_pos = match s.assign.as_ref() {
                            Stmt::AssignStmt(a) => a
                                .lhs
                                .first()
                                .map(|e| e.pos().0 as u32)
                                .unwrap_or_else(|| s.assign.pos().0 as u32),
                            _ => s.assign.pos().0 as u32,
                        };
                        let typ = case_t.unwrap_or_else(|| self.invalid_type());
                        let obj = new_var(&mut self.objects, name.clone(), typ);
                        obj.set_pkg(&mut self.objects, self.pkg);
                        obj.set_pos(&mut self.objects, lhs_pos);
                        if let ObjectData::Var(v) = self.objects.get_mut(obj) {
                            v.set_kind(VarKind::Local);
                        }
                        let scope = self.env.scope.expect("case scope");
                        self.declare(scope, obj, clause.colon.0 as u32);
                        // Record the case-specific narrowed variable as the
                        // clause's implicit object (Go `recordImplicit`).
                        self.record_implicit(clause.id, obj);
                        clause_binding_objs.push(obj);
                    }
                    self.stmt_list(inner, &clause.body);
                    self.close_scope();
                }

                if let Some(name) = &lhs_name {
                    if !clause_binding_objs.is_empty() {
                        let any_used = clause_binding_objs
                            .iter()
                            .any(|obj| self.used_vars.contains(obj));
                        if !any_used {
                            let lhs_pos = match s.assign.as_ref() {
                                Stmt::AssignStmt(a) => a
                                    .lhs
                                    .first()
                                    .map(|e| e.pos().0 as u32)
                                    .unwrap_or_else(|| s.assign.pos().0 as u32),
                                _ => s.assign.pos().0 as u32,
                            };
                            self.error(
                                lhs_pos,
                                Code::UnusedVar,
                                format!("declared and not used: {}", name),
                            );
                        } else {
                            for obj in clause_binding_objs {
                                self.used_vars.insert(obj);
                            }
                        }
                    }
                }

                self.close_scope();
            }

            // ===== DEFERRED dispatch (forward pointers) =====
            // The labels.go 2nd pass (LabeledStmt resolution / hasLabel).
            _ => {
                // DEFERRED: not yet checked. Leaving unhandled is safe (no
                // false positives); these land in later sub-chunks.
            }
        }

        self.process_delayed(top);
    }
}
