//! `use-slices-concat` (revive v1.17.0) — appends into an empty slice literal
//! that a single `slices.Concat` call would replace: nested
//! (`append(append([]T{}, a...), b...)`) or consecutive
//! (`s := append([]T{}, a...)` then `s = append(s, b...)`).

use guff::ast::{AssignStmt, CallExpr, Expr, Stmt};
use guff::token::Token;
use guff::walk::{self, expr_ref, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{go_version_at_least, is_ident};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    if !go_version_at_least(pass, 1, 22) {
        return Vec::new();
    }
    let mut failures = Vec::new();
    for file in pass.files() {
        visit(NodeRef::File(file), &mut failures);
    }
    failures
}

fn visit(n: NodeRef<'_>, failures: &mut Vec<Failure>) {
    walk::inspect(n, |n| {
        let Some(n) = n else { return true };
        match n {
            NodeRef::BlockStmt(b) => check_consecutive_appends(&b.list, failures),
            NodeRef::CaseClause(c) => check_consecutive_appends(&c.body, failures),
            NodeRef::CommClause(c) => check_consecutive_appends(&c.body, failures),
            NodeRef::CallExpr(call) => {
                let appended = appended_slices_call(call);
                if appended.len() > 1 && appended[1..].iter().all(|e| is_side_effect_free(e)) {
                    failures.push(failure(
                        call.pos().0 as u32,
                        "replace nested appends by a call to slices.Concat",
                    ));
                    // Only the appended slices, so the nested appends are not
                    // reported again.
                    for slice in appended {
                        visit(expr_ref(slice), failures);
                    }
                    return false;
                }
            }
            _ => {}
        }
        true
    });
}

fn failure(pos: u32, msg: &str) -> Failure {
    Failure::with_confidence("use-slices-concat", pos, msg, 0.8)
}

/// `checkConsecutiveAppends`.
fn check_consecutive_appends(stmts: &[Stmt], failures: &mut Vec<Failure>) {
    for pair in stmts.windows(2) {
        let Some(name) = append_to_empty_slice_target(&pair[0]) else {
            continue;
        };
        if appends_to_target(&pair[1], name) {
            failures.push(failure(
                pair[0].pos().0 as u32,
                "replace consecutive appends by a call to slices.Concat",
            ));
        }
    }
}

fn single_assign(stmt: &Stmt, tok: Token) -> Option<&AssignStmt> {
    let Stmt::AssignStmt(a) = stmt else { return None };
    (a.tok == Some(tok) && a.lhs.len() == 1 && a.rhs.len() == 1).then_some(a)
}

/// `appendToEmptySliceTarget`: `s := append([]T{}, s1...)`.
fn append_to_empty_slice_target(stmt: &Stmt) -> Option<&str> {
    let a = single_assign(stmt, Token::DEFINE)?;
    let Expr::Ident(target) = &a.lhs[0] else { return None };
    if target.name == "_" {
        return None;
    }
    // More than one nested append is already reported as nested appends.
    (appended_slices(&a.rhs[0]).len() == 1).then_some(target.name.as_str())
}

/// `appendsToTarget`: `target = append(target, s...)`.
fn appends_to_target(stmt: &Stmt, target: &str) -> bool {
    let Some(a) = single_assign(stmt, Token::ASSIGN) else {
        return false;
    };
    if !is_ident(&a.lhs[0], target) {
        return false;
    }
    let Expr::CallExpr(call) = &a.rhs[0] else {
        return false;
    };
    if !is_variadic_append(call) || !is_ident(&call.args[0], target) {
        return false;
    }
    // `target = append(target, f(target)...)` cannot become slices.Concat:
    // the target is not defined yet in the replacement.
    !uses_ident(&call.args[1], target) && is_side_effect_free(&call.args[1])
}

/// `isSideEffectFree`: the appended slices are copied only once all of them
/// are evaluated, so one that could mutate another must not be reported.
fn is_side_effect_free(e: &Expr) -> bool {
    match e {
        Expr::Ident(_) | Expr::BasicLit(_) => true,
        Expr::ParenExpr(p) => is_side_effect_free(&p.x),
        Expr::StarExpr(s) => is_side_effect_free(&s.x),
        Expr::SelectorExpr(s) => is_side_effect_free(&s.x),
        Expr::IndexExpr(ix) => is_side_effect_free(&ix.x) && is_side_effect_free(&ix.index),
        Expr::SliceExpr(s) => {
            is_side_effect_free(&s.x)
                && [&s.low, &s.high, &s.max]
                    .into_iter()
                    .all(|b| b.as_deref().is_none_or(is_side_effect_free))
        }
        // Only a conversion to a slice type, `[]byte("a string")`, is not a call.
        Expr::CallExpr(c) => {
            matches!(&*c.fun, Expr::ArrayType(_))
                && c.args.len() == 1
                && is_side_effect_free(&c.args[0])
        }
        _ => false,
    }
}

/// `usesIdent`.
fn uses_ident(e: &Expr, name: &str) -> bool {
    let mut found = false;
    walk::inspect(expr_ref(e), |n| {
        if let Some(NodeRef::Ident(id)) = n {
            found |= id.name == name;
        }
        !found
    });
    found
}

/// `appendedSlices`: `[s1, s2]` for `append(append([]T{}, s1...), s2...)`,
/// empty when the expression is not of that form.
fn appended_slices(e: &Expr) -> Vec<&Expr> {
    match e {
        Expr::CallExpr(call) => appended_slices_call(call),
        _ => Vec::new(),
    }
}

fn appended_slices_call(call: &CallExpr) -> Vec<&Expr> {
    if !is_variadic_append(call) {
        return Vec::new();
    }
    if is_empty_slice_literal(&call.args[0]) {
        return vec![&call.args[1]];
    }
    let mut nested = appended_slices(&call.args[0]);
    if nested.is_empty() {
        return nested;
    }
    nested.push(&call.args[1]);
    nested
}

/// `isVariadicAppend`: `append(s1, s2...)`, except a string literal spread
/// into a byte slice, which `slices.Concat` does not accept.
fn is_variadic_append(call: &CallExpr) -> bool {
    is_ident(&call.fun, "append")
        && call.ellipsis.is_valid()
        && call.args.len() == 2
        && !matches!(&call.args[1], Expr::BasicLit(l) if l.kind == Some(Token::STRING))
}

fn is_empty_slice_literal(e: &Expr) -> bool {
    let Expr::CompositeLit(lit) = e else { return false };
    lit.elts.is_empty()
        && matches!(lit.ty.as_deref(), Some(Expr::ArrayType(at)) if at.len.is_none())
}
