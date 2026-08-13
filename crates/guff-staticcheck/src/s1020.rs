//! S1020 — omit redundant nil check in type assertion.
//!
//! Port of `honnef.co/go/tools/simple/s1020`.
//!
//! **Parentheses.** Upstream states this check as a `pattern` query, and
//! `pattern.match` strips `*ast.ParenExpr` at every recursion (before binding),
//! so `f((x))` matches wherever `f(x)` does. This port descends by hand, so
//! every descent has to `unparen` — `compat/fuzz.py`'s `paren` mutation found
//! nine S-checks going quiet on a parenthesized subexpression at once
//! (COMPAT-HARDENING §4, 2026-08-13).

use std::sync::OnceLock;

use guff::ast::{AssignStmt, BinaryExpr, Expr, Ident, IfStmt, Stmt, TypeAssertExpr};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{is_nil, unparen};
use guff_analysis::passes::inspect;
use crate::render::render_expr;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn type_assert_init(init: &Stmt) -> Option<(&Ident, &TypeAssertExpr)> {
    let Stmt::AssignStmt(AssignStmt { lhs, rhs, tok, .. }) = init else {
        return None;
    };
    if tok != &Some(Token::DEFINE) || lhs.len() != 2 || rhs.len() != 1 {
        return None;
    };
    let Expr::Ident(blank) = unparen(&lhs[0]) else {
        return None;
    };
    if blank.name != "_" {
        return None;
    };
    let Expr::Ident(ok) = unparen(&lhs[1]) else {
        return None;
    };
    let Expr::TypeAssertExpr(ta) = unparen(&rhs[0]) else {
        return None;
    };
    Some((ok, ta))
}

fn is_redundant_nil_cond(pass: &Pass<'_>, cond: &Expr, ok: &Ident, assert_expr: &Expr) -> bool {
    let Expr::Ident(asserted) = unparen(assert_expr) else {
        return false;
    };
    let Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) = unparen(cond) else {
        return false;
    };
    if *op != Token::LAND {
        return false;
    }
    let assert_neq_nil = |e: &Expr| {
        let Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) = unparen(e) else {
            return false;
        };
        *op == Token::NEQ
            && matches!(unparen(x), Expr::Ident(lhs) if lhs.name == asserted.name)
            && is_nil(pass, y)
    };
    match (unparen(x), unparen(y)) {
        (Expr::Ident(id), rhs) if id.name == ok.name => assert_neq_nil(rhs),
        (lhs, Expr::Ident(id)) if id.name == ok.name => assert_neq_nil(lhs),
        _ => false,
    }
}

/// `(ok's name, the asserted name)` — upstream prints both objects' names
/// (`when %s is true, %s can't be nil`), and the first one was hardcoded here as
/// the literal `ok`. Every fixture in the tree happens to call it `ok`, so the
/// message was right until `compat/fuzz.py`'s `rename` mutation called it
/// `err` and guff kept saying `ok`.
fn check_if(pass: &Pass<'_>, ifs: &IfStmt) -> Option<(String, String)> {
    let init = ifs.init.as_deref().and_then(type_assert_init);
    let (ok, ta) = init?;
    if !is_redundant_nil_cond(pass, &ifs.cond, ok, &ta.x) {
        return None;
    }
    Some((ok.name.clone(), render_expr(&ta.x)))
}

fn check_nested_if(pass: &Pass<'_>, ifs: &IfStmt) -> Option<(String, String)> {
    if ifs.init.is_some() {
        return None;
    }
    let Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) = unparen(&ifs.cond) else {
        return None;
    };
    if *op != Token::NEQ || !is_nil(pass, y) {
        return None;
    };
    let Expr::Ident(lhs) = unparen(x) else {
        return None;
    };
    if ifs.body.list.len() != 1 {
        return None;
    };
    let Stmt::IfStmt(inner) = &ifs.body.list[0] else {
        return None;
    };
    let (ok, ta) = inner.init.as_deref().and_then(type_assert_init)?;
    if ta.x.id() != lhs.id() && !matches!((unparen(&ta.x), unparen(x)), (Expr::Ident(a), Expr::Ident(b)) if a.name == b.name) {
        return None;
    }
    if !matches!(unparen(&inner.cond), Expr::Ident(id) if id.name == ok.name) {
        return None;
    }
    Some((ok.name.clone(), render_expr(&ta.x)))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1020 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(IfStmt), pass.files(), |node| {
        let NodeRef::IfStmt(ifs) = node else {
            return;
        };
        // Upstream names the asserted expression rather than describing it.
        if let Some((ok, value)) = check_if(pass, ifs).or_else(|| check_nested_if(pass, ifs)) {
            pending.push((
                match_pos(node),
                format!("when {ok} is true, {value} can't be nil"),
            ));
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1020_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1020",
        doc: "omit redundant nil check in type assertion",
        url: "https://staticcheck.dev/docs/checks/#S1020",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1020_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1020_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
