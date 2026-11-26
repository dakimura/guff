//! Port of `return.go` — `isTerminating` and the `hasBreak` helpers, used to
//! diagnose a missing `return` at the end of a function with results.
//!
//! These are pure structural predicates over the (go/ast-shaped) statement
//! tree, so they are free functions rather than `Checker` methods.
//!
//! ## Simplifications
//!
//! - **`panic` detection**: Go records each checked call to the predeclared
//!   `panic` in `check.isPanic`. We instead recognise a call whose callee is
//!   the identifier `panic` structurally; this does not account for a
//!   user-defined `panic` shadowing the builtin (very rare).
//! - The unreachable `panic("unreachable")` defaults in Go's `switch`es become
//!   a conservative `false` (treat unknown statements as non-terminating).

use guff::ast::{Expr, Stmt};
use guff::token::Token;

use crate::stmt::unparen;

/// Reports whether `s` is a terminating statement. `label` is the label of `s`
/// if it is labeled, else `""`.
///
/// Equivalent to `Checker.isTerminating`.
pub fn is_terminating(s: &Stmt, label: &str) -> bool {
    match s {
        Stmt::LabeledStmt(l) => is_terminating(&l.stmt, l.label.name.as_str()),

        Stmt::ExprStmt(e) => {
            // A call to the predeclared `panic` is terminating.
            if let Expr::CallExpr(call) = unparen(&e.x) {
                if is_panic_call(&call.fun) {
                    return true;
                }
            }
            false
        }

        Stmt::ReturnStmt(_) => true,

        Stmt::BranchStmt(b) => {
            matches!(b.tok, Token::GOTO | Token::FALLTHROUGH)
        }

        Stmt::BlockStmt(b) => is_terminating_list(&b.list, ""),

        Stmt::IfStmt(s) => {
            if let Some(else_) = &s.else_ {
                let then_block = Stmt::BlockStmt(s.body.clone());
                is_terminating(&then_block, "") && is_terminating(else_, "")
            } else {
                false
            }
        }

        // go/ast splits expr- and type-switches; both terminate under the same
        // rule (a default clause plus every clause terminating w/o break).
        Stmt::SwitchStmt(s) => is_terminating_switch(&s.body.list, label),
        Stmt::TypeSwitchStmt(s) => is_terminating_switch(&s.body.list, label),

        Stmt::SelectStmt(s) => {
            for c in &s.body.list {
                if let Stmt::CommClause(cc) = c {
                    if !is_terminating_list(&cc.body, "") || has_break_list(&cc.body, label, true) {
                        return false;
                    }
                }
            }
            true
        }

        Stmt::ForStmt(s) => {
            // A `for {}` with no condition and no break is terminating.
            s.cond.is_none() && !has_break_list(&s.body.list, label, true)
        }

        // RangeStmt is never terminating (the loop may run zero times —
        // go.dev/issue/49003). All other statements: no chance.
        _ => false,
    }
}

/// Reports whether the last non-empty statement of `list` is terminating.
///
/// Equivalent to `Checker.isTerminatingList`.
pub fn is_terminating_list(list: &[Stmt], label: &str) -> bool {
    for s in list.iter().rev() {
        if !matches!(s, Stmt::EmptyStmt(_)) {
            return is_terminating(s, label);
        }
    }
    false // all statements are empty
}

/// Reports whether a `switch`/type-switch whose clauses are `body` is
/// terminating: it must have a `default` clause and every clause must be
/// terminating without an applicable `break`.
///
/// Equivalent to `Checker.isTerminatingSwitch`.
fn is_terminating_switch(body: &[Stmt], label: &str) -> bool {
    let mut has_default = false;
    for c in body {
        if let Stmt::CaseClause(cc) = c {
            if cc.list.is_empty() {
                has_default = true;
            }
            if !is_terminating_list(&cc.body, "") || has_break_list(&cc.body, label, true) {
                return false;
            }
        }
    }
    has_default
}

/// Reports whether `call_fun` is (a parenthesized) reference to the predeclared
/// `panic`. Structural approximation of `check.isPanic` (see module docs).
fn is_panic_call(call_fun: &Expr) -> bool {
    matches!(unparen(call_fun), Expr::Ident(id) if id.name == "panic")
}

/// Reports whether `s` is, or contains, a `break` referring to the `label`-ed
/// statement (or, when `implicit`, the closest enclosing breakable statement).
///
/// Equivalent to `hasBreak`.
pub fn has_break(s: &Stmt, label: &str, implicit: bool) -> bool {
    match s {
        Stmt::LabeledStmt(l) => has_break(&l.stmt, label, implicit),

        Stmt::BranchStmt(b) if b.tok == Token::BREAK => match &b.label {
            None => implicit,
            Some(l) => l.name == label,
        },

        Stmt::BlockStmt(b) => has_break_list(&b.list, label, implicit),

        Stmt::IfStmt(s) => {
            let then_block = Stmt::BlockStmt(s.body.clone());
            has_break(&then_block, label, implicit)
                || s.else_
                    .as_ref()
                    .is_some_and(|e| has_break(e, label, implicit))
        }

        // For an inner breakable statement, only a *labeled* break can escape it.
        Stmt::SwitchStmt(s) => !label.is_empty() && has_break_case_list(&s.body.list, label, false),
        Stmt::TypeSwitchStmt(s) => {
            !label.is_empty() && has_break_case_list(&s.body.list, label, false)
        }
        Stmt::SelectStmt(s) => !label.is_empty() && has_break_comm_list(&s.body.list, label, false),
        Stmt::ForStmt(s) => !label.is_empty() && has_break_list(&s.body.list, label, false),
        Stmt::RangeStmt(s) => !label.is_empty() && has_break_list(&s.body.list, label, false),

        _ => false,
    }
}

/// Equivalent to `hasBreakList`.
pub fn has_break_list(list: &[Stmt], label: &str, implicit: bool) -> bool {
    list.iter().any(|s| has_break(s, label, implicit))
}

/// Equivalent to `hasBreakCaseList` (over `CaseClause` bodies).
fn has_break_case_list(body: &[Stmt], label: &str, implicit: bool) -> bool {
    body.iter().any(|c| match c {
        Stmt::CaseClause(cc) => has_break_list(&cc.body, label, implicit),
        _ => false,
    })
}

/// Equivalent to `hasBreakCommList` (over `CommClause` bodies).
fn has_break_comm_list(body: &[Stmt], label: &str, implicit: bool) -> bool {
    body.iter().any(|c| match c {
        Stmt::CommClause(cc) => has_break_list(&cc.body, label, implicit),
        _ => false,
    })
}
