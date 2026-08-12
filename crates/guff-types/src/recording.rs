//! Recording of type information into [`crate::api::Info`], ported from
//! `cmd/compile/internal/types2/recording.go`.
//!
//! Go keys the `Info` maps on AST-node pointers (`*syntax.Name`,
//! `*syntax.Expr`, …). This port cannot: the type checker clones
//! `Expr`/`Ident` values as it works (e.g. when capturing closures for
//! `Checker::later`), so two values that denote the same source node are
//! distinct allocations. Instead, the parser stamps every identifier with a
//! process-unique [`id`](guff::ast::Ident::id) (see
//! `guff::ast::next_node_id`), clones inherit it, and the maps key on that
//! id. An id of `0` marks a hand-built / synthetic node and is never recorded
//! (matching Go's "omit" behaviour for nodes the checker never visits).
//!
//! ## Scope (chunks 49–50)
//!
//! - chunk 49 ported `Defs`/`Uses` (`recordDef`/`recordUse`), keyed on
//!   `*syntax.Name` (only need identifier identity). They are the workhorses of
//!   every downstream analysis pass (`pass.TypesInfo.Uses[ident]`).
//! - chunk 50 ports **`Types` (`recordTypeAndValue`)**, keyed on *every*
//!   `Expr`. This is unlocked by stamping a stable id onto every `Expr` variant
//!   (see [`crate::stamp`](guff::stamp) — the post-parse pass). [`record`]
//!   is the per-expression dispatcher (Go's `Checker.record`), called from
//!   [`Checker::raw_expr`](crate::Checker) and the type-expression checker.
//!   [`record_builtin_type`](Checker::record_builtin_type) records a builtin's
//!   synthesised signature.
//!
//! ### Untyped delay (chunk 51)
//!
//! Go does not record *untyped* expressions immediately: it stashes them in
//! `check.untyped` ([`remember_untyped`](Checker::remember_untyped)) and only
//! commits a final `Types` entry once the type is known. Two things narrow an
//! untyped entry:
//!
//! - [`update_expr_type`](Checker::update_expr_type) — invoked from
//!   `convert_untyped` when an untyped operand is materialised into a context
//!   type (e.g. the `1 + 2` and its operands in `var x int = 1 + 2` become
//!   `int`), and from `comparison` to pin each operand to its default type.
//! - [`record_untyped`](Checker::record_untyped) — the end-of-check flush
//!   (Go's tail `check.recordUntyped()`); whatever is still untyped is recorded
//!   with its (untyped) type, e.g. the operands of a folded constant.
//!
//! This matches Go faithfully: in `var x int = 1 + 2`, the *whole* expression
//! `1 + 2` is recorded as `int`, while the operand literals `1` and `2` —
//! never materialised individually (a constant binary expr does not recurse
//! into its operands) — stay `untyped int`.
//!
//! ### Comma-ok promotion (chunk 52)
//!
//! When a map index / type assertion / channel receive is used in a two-value
//! context (`v, ok := m[k]`), its single recorded value type is promoted to a
//! 2-tuple `(t0, t1)` by [`record_comma_ok_types`](Checker::record_comma_ok_types)
//! (Go's `Checker.recordCommaOkTypes`), wired from `init_vars` / `assign_vars`.
//!
//! ### Selections & Instances (chunk 53)
//!
//! - [`record_selection`](Checker::record_selection) (Go's `recordSelection`)
//!   populates `Info.Selections` for every field/method selector `x.f`, wired
//!   from the three branches of `call.rs::selector` and from
//!   `builtins.rs::builtin_offsetof`. Its unconditional `recordUse(x.Sel, obj)`
//!   side effect (previously the only part wired) now lives inside it.
//! - [`record_instance`](Checker::record_instance) (Go's `recordInstance`)
//!   populates `Info.Instances`, wired from `typexpr.rs::instantiated_type`
//!   (`T[…]`) and `call.rs::infer_call` (inferred generic calls `F(…)`).
//!
//! ## Deferrals
//!
//! - **Labels** — no `Label` object exists (`labels.rs` is name-based), so
//!   label defs are not recorded. Anonymous parameters use
//!   [`record_implicit`](Checker::record_implicit) (chunk 72).
//! - **`ParenExpr` inner node / `NoValue` tuple type** — `expr_internal`
//!   recurses into a parenthesised expression directly (not through
//!   `raw_expr`), so only the outer `ParenExpr` is recorded. And a `NoValue`
//!   operand is recorded with `Typ[Invalid]` rather than Go's `(*Tuple)(nil)`
//!   (callers distinguish it by `mode`).

use guff::ast::{Expr, Ident, SelectorExpr};
use guff::token::Token;

use crate::api::{Instance, TypeAndValue};
use crate::arena::{ObjectData, ObjectId, TypeId};
use crate::check::Checker;
use crate::object::var::{new_var, VarKind};
use crate::operand::{Operand, OperandMode};
use crate::predicates::is_untyped;
use crate::selection::{Selection, SelectionKind};
use guff_constant::Value;
use guff_types_errors::Code;

/// Information about an untyped expression awaiting its final type.
///
/// Equivalent to Go's `exprInfo`. Stored in [`Checker::untyped`], keyed on the
/// expression's stable AST node id.
#[derive(Debug, Clone)]
pub struct ExprInfo {
    /// The expression is the lhs operand of a non-constant shift whose type
    /// check is delayed (Go's `isLhs`). When the operand materialises its
    /// final type must be an integer.
    pub is_lhs: bool,
    /// The operand mode recorded for the expression.
    pub mode: OperandMode,
    /// The (untyped) type of the expression. Always an untyped `Basic`.
    pub typ: TypeId,
    /// The constant value, if the expression is a constant.
    pub val: Option<Value>,
}

impl Checker {
    /// Records that identifier `id` *defines* `obj` (Go's `Checker.recordDef`).
    ///
    /// `obj` is `None` for identifiers that denote no object (e.g. the blank
    /// `_`, or a file's package name — currently never recorded). Stamped
    /// identifiers only (`id.id() != 0`).
    pub fn record_def(&mut self, id: &Ident, obj: Option<ObjectId>) {
        if !self.record_info {
            return;
        }
        let nid = id.id();
        if nid == 0 {
            return;
        }
        self.info.defs.insert(nid, obj);
    }

    /// Records that identifier `id` *denotes* the already-declared `obj`
    /// (Go's `Checker.recordUse`). Stamped identifiers only.
    pub fn record_use(&mut self, id: &Ident, obj: ObjectId) {
        if !self.record_info {
            return;
        }
        let nid = id.id();
        if nid == 0 {
            return;
        }
        self.info.uses.insert(nid, obj);
    }

    /// Record the selection `x.f` denoted by selector `e` (Go's
    /// `Checker.recordSelection`).
    ///
    /// As in Go, this also records the *use* of `e.sel` (`recordUse(x.Sel,
    /// obj)`) — the one effect that is unconditional, independent of whether the
    /// `Selections` map is populated. `recv` is the type of the operand `x`,
    /// `index` the embedded-field path, and `indirect` whether any pointer
    /// indirection was traversed. Stamped selectors only.
    pub fn record_selection(
        &mut self,
        e: &SelectorExpr,
        kind: SelectionKind,
        recv: TypeId,
        obj: ObjectId,
        index: Vec<i32>,
        indirect: bool,
    ) {
        self.record_use(&e.sel, obj);
        if !self.record_info || e.id == 0 {
            return;
        }
        self.info
            .selections
            .insert(e.id, Selection::new(kind, recv, obj, index, indirect));
    }

    /// Record that the generic type/function denoted by `expr` was instantiated
    /// with `targs`, yielding `typ` (Go's `Checker.recordInstance`).
    ///
    /// The entry is keyed on the *instantiated identifier* — the leading name
    /// of `expr` (see [`instantiated_ident`]). `expr` here is the operand being
    /// instantiated (the `x` of an `IndexExpr`/`IndexListExpr`), matching the
    /// call sites in `typexpr.rs` / `call.rs`. Synthetic / unstamped idents are
    /// dropped.
    pub fn record_instance(&mut self, expr: &Expr, targs: Vec<TypeId>, typ: TypeId) {
        if !self.record_info {
            return;
        }
        let ident = match instantiated_ident(expr) {
            Some(id) => id,
            None => return, // Go panics ("not found"); be defensive instead.
        };
        if ident.id() == 0 {
            return;
        }
        self.info.instances.insert(
            ident.id(),
            Instance {
                type_args: targs,
                typ,
            },
        );
    }

    /// Convert operand `x` (evaluated for expression `e`) into a user-friendly
    /// `(type, value)` pair and record it in `Info.Types` (Go's
    /// `Checker.record`).
    ///
    /// If `e`'s type is still untyped, recording is delayed via
    /// [`remember_untyped`](Checker::remember_untyped) until the final type is
    /// known (or until the end of checking).
    pub fn record(&mut self, x: &Operand, e: &Expr) {
        let (typ, val) = match x.mode {
            OperandMode::Invalid => (self.invalid_type(), None),
            // Go uses the nil `*Tuple` type for a no-value operand; we have no
            // such representation (the empty tuple is `None`), so we store
            // `Typ[Invalid]` and rely on `mode == NoValue` to distinguish it.
            OperandMode::NoValue => (self.invalid_type(), None),
            OperandMode::Constant => (x.typ.unwrap_or_else(|| self.invalid_type()), x.val.clone()),
            _ => (x.typ.unwrap_or_else(|| self.invalid_type()), None),
        };
        if is_untyped(&self.types, typ) {
            // Delay type and value recording until we know the type or until
            // the end of type checking.
            self.remember_untyped(e, false, x.mode, typ, val);
        } else {
            self.record_type_and_value(e, x.mode, typ, val);
        }
    }

    /// Stash an untyped expression `e` for later recording (Go's
    /// `Checker.rememberUntyped`). Synthetic / unstamped nodes (id `0`) cannot
    /// be keyed and are dropped — they are never visited by `update_expr_type`
    /// and need no `Types` entry.
    pub fn remember_untyped(
        &mut self,
        e: &Expr,
        lhs: bool,
        mode: OperandMode,
        typ: TypeId,
        val: Option<Value>,
    ) {
        let nid = e.id();
        if nid == 0 {
            return;
        }
        self.untyped.insert(
            nid,
            ExprInfo {
                is_lhs: lhs,
                mode,
                typ,
                val,
            },
        );
    }

    /// Flush every expression still in [`Checker::untyped`] into `Info.Types`,
    /// keyed by its (untyped) type (Go's `Checker.recordUntyped`). Called once
    /// at the end of `check_files`.
    pub fn record_untyped(&mut self) {
        if !self.record_info {
            // Go: `if !check.recordTypes() { return }`. Drop the stash so we
            // don't retain it until the checker is freed.
            self.untyped.clear();
            return;
        }
        let entries: Vec<(u32, ExprInfo)> = self.untyped.drain().collect();
        for (nid, info) in entries {
            if info.mode == OperandMode::Invalid {
                continue; // omit (matches record_type_and_value)
            }
            // The id (a stamped, non-zero node id) is the same key
            // record_type_and_value would derive from the Expr we no longer
            // hold, so insert directly.
            self.info.types.insert(
                nid,
                TypeAndValue {
                    mode: info.mode,
                    typ: info.typ,
                    val: info.val,
                },
            );
        }
    }

    /// Update the value of `e`'s untyped entry to `val` (Go's
    /// `Checker.updateExprVal`). No-op if `e` is not (or no longer) untyped.
    pub fn update_expr_val(&mut self, e: &Expr, val: Value) {
        if let Some(info) = self.untyped.get_mut(&e.id()) {
            info.val = Some(val);
        }
    }

    /// Update the type of `e` to `typ`, recursing into its operands as needed,
    /// and — once `typ` is final (typed, or `final_` is set) — commit the
    /// `Types` entry (Go's `Checker.updateExprType`).
    ///
    /// If `typ` is still untyped and `!final_`, only the recorded untyped type
    /// is narrowed. Otherwise the entry is removed from [`Checker::untyped`]
    /// and recorded; a formerly-untyped shift-lhs operand must end up integer,
    /// and a constant must be representable as `typ`.
    pub fn update_expr_type(&mut self, e: &Expr, typ: TypeId, final_: bool) {
        let nid = e.id();
        let old = match self.untyped.get(&nid) {
            Some(info) => info.clone(),
            None => return, // nothing to do
        };

        // Update operands of `e` if necessary. Constant expressions do not
        // recurse: their operands are never materialised and, if left in the
        // map, are flushed by `record_untyped`.
        match e {
            Expr::ParenExpr(p) => self.update_expr_type(&p.x, typ, final_),
            Expr::UnaryExpr(u) => {
                if old.val.is_none() {
                    self.update_expr_type(&u.x, typ, final_);
                }
            }
            Expr::BinaryExpr(b) => {
                if old.val.is_none() {
                    if is_comparison_token(b.op) {
                        // Result type is independent of operand types, which
                        // already have their final types.
                    } else if is_shift_token(b.op) {
                        // Result type depends only on the lhs operand; the rhs
                        // type was set when checking the shift.
                        self.update_expr_type(&b.x, typ, final_);
                    } else {
                        self.update_expr_type(&b.x, typ, final_);
                        self.update_expr_type(&b.y, typ, final_);
                    }
                }
            }
            // Ident / BasicLit / SelectorExpr / CallExpr: no operands to fix.
            // Every other expression form is never untyped — nothing to do.
            _ => {}
        }

        // If the new type is not final and still untyped, just update the
        // recorded type (untyped basics are their own underlying).
        if !final_ && is_untyped(&self.types, typ) {
            let mut updated = old;
            updated.typ = typ;
            self.untyped.insert(nid, updated);
            return;
        }

        // Otherwise we have the final type. Remove `e` from the untyped map.
        self.untyped.remove(&nid);

        if old.is_lhs
            && !crate::predicates::all_integer(&mut self.types, &self.objects, &self.packages, typ)
        {
            // The lhs of a non-constant shift must end up an integer type.
            let es = self.type_str(typ);
            self.error(
                e.pos().0 as u32,
                Code::InvalidShiftOperand,
                format!("shifted operand (type {}) must be integer", es),
            );
            return;
        }
        if let Some(val) = &old.val {
            // A constant must be representable as a value of `typ`. The map
            // entry is already gone, so the recursive update_expr_type inside
            // convert_untyped is a no-op (matches Go).
            let mut c = Operand {
                mode: old.mode,
                expr: Some(e),
                typ: Some(old.typ),
                val: Some(val.clone()),
                id: None,
            };
            self.convert_untyped(&mut c, typ);
            if c.mode == OperandMode::Invalid {
                return;
            }
        }

        self.record_type_and_value(e, old.mode, typ, old.val);
    }

    /// Record the type (and, for constants, value) of expression `e` in
    /// `Info.Types` (Go's `Checker.recordTypeAndValue`).
    ///
    /// `mode == Invalid` is omitted (Go does the same). Synthetic expressions
    /// (id `0`, never stamped by the parser) are likewise omitted, since there
    /// is no stable key for them.
    pub fn record_type_and_value(
        &mut self,
        e: &Expr,
        mode: OperandMode,
        typ: TypeId,
        val: Option<Value>,
    ) {
        if !self.record_info {
            return;
        }
        if mode == OperandMode::Invalid {
            return; // omit
        }
        let nid = e.id();
        if nid == 0 {
            return; // synthetic / unstamped node — nothing to key on
        }
        self.info.types.insert(nid, TypeAndValue { mode, typ, val });
    }

    /// Record the signature `sig` for the (possibly parenthesised) identifier
    /// or selector `f` denoting a built-in (Go's `Checker.recordBuiltinType`).
    pub fn record_builtin_type(&mut self, f: &Expr, sig: TypeId) {
        let mut cur = f;
        loop {
            self.record_type_and_value(cur, OperandMode::Builtin, sig, None);
            match cur {
                Expr::Ident(_) | Expr::SelectorExpr(_) => return,
                Expr::ParenExpr(p) => cur = &p.x,
                // Go panics ("unreachable") here; be defensive instead.
                _ => return,
            }
        }
    }

    /// Promote the recorded type of comma-ok expression `e` (and any
    /// parenthesised parents) to the 2-tuple `(t0, t1)`, reflecting that `e`
    /// is used in a two-value context (Go's `Checker.recordCommaOkTypes`).
    ///
    /// `t0` is the value type, `t1` the boolean "ok" (or `error`) type; both
    /// are already typed. The single-value `Types` entry recorded for `e` when
    /// it was first checked is rewritten in place — its `mode` and `val` are
    /// preserved, only `typ` changes. Defensive about a missing entry (Go
    /// asserts one exists); `recordCommaOkTypesInSyntax` is not ported.
    pub fn record_comma_ok_types(&mut self, e: &Expr, t0: TypeId, t1: TypeId) {
        // Only mutates Info.Types (plus throwaway arena temps for the 2-tuple).
        // Skip entirely when Info is discarded — avoids overlay TypeId noise.
        if !self.record_info {
            return;
        }
        // Build the 2-tuple (var t0, var t1) once (Go uses unnamed LocalVars).
        let v0 = new_var(&mut self.objects, "", t0);
        let v1 = new_var(&mut self.objects, "", t1);
        for v in [v0, v1] {
            v.set_pkg(&mut self.objects, self.pkg);
            if let ObjectData::Var(var) = self.objects.get_mut(v) {
                var.set_kind(VarKind::Local);
            }
        }
        let tup = match crate::tuple::new_tuple(&mut self.types, &[v0, v1]) {
            Some(t) => t,
            None => return,
        };

        let mut cur = e;
        loop {
            if let Some(tv) = self.info.types.get_mut(&cur.id()) {
                tv.typ = tup;
            }
            match cur {
                Expr::ParenExpr(p) => cur = &p.x,
                _ => break,
            }
        }
    }
}

/// Whether `op` is a comparison operator (mirrors `expr.rs::is_comparison_op`,
/// duplicated here to keep `update_expr_type`'s operand recursion local).
fn is_comparison_token(op: Token) -> bool {
    matches!(
        op,
        Token::EQL | Token::NEQ | Token::LSS | Token::LEQ | Token::GTR | Token::GEQ
    )
}

/// Whether `op` is a shift operator (mirrors `expr.rs::is_shift_op`).
fn is_shift_token(op: Token) -> bool {
    matches!(op, Token::SHL | Token::SHR)
}

/// The identifier of the type/function being instantiated in `expr` (Go's
/// `instantiatedIdent`, adapted).
///
/// Go is passed the whole instantiation expression (an `IndexExpr` /
/// `IndexListExpr`, or a bare ident/selector for inferred function calls) and
/// digs out the leading name. Our `record_instance` call sites already pass the
/// *operand* being instantiated (the `x` of the index expr, or the callee
/// expression), so here `expr` is that operand directly:
///
/// - `Ident` → that identifier.
/// - `SelectorExpr` (`pkg.T`) → the selector's `sel`.
///
/// Returns `None` for any other shape (Go panics with a "please report"
/// diagnostic; we stay defensive).
fn instantiated_ident(expr: &Expr) -> Option<&Ident> {
    match expr {
        Expr::Ident(id) => Some(id),
        Expr::SelectorExpr(s) => Some(&s.sel),
        _ => None,
    }
}
