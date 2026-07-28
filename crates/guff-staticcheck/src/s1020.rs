//! S1020 — omit redundant nil check in type assertion.
//!
//! Port of `honnef.co/go/tools/simple/s1020`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, BinaryExpr, Expr, Ident, IfStmt, Stmt, TypeAssertExpr};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::is_nil;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn type_assert_init(init: &Stmt) -> Option<(&Ident, &TypeAssertExpr)> {
    let Stmt::AssignStmt(AssignStmt { lhs, rhs, tok, .. }) = init else {
        return None;
    };
    if tok != &Some(Token::DEFINE) || lhs.len() != 2 || rhs.len() != 1 {
        return None;
    };
    let Expr::Ident(blank) = &lhs[0] else {
        return None;
    };
    if blank.name != "_" {
        return None;
    };
    let Expr::Ident(ok) = &lhs[1] else {
        return None;
    };
    let Expr::TypeAssertExpr(ta) = &rhs[0] else {
        return None;
    };
    Some((ok, ta))
}

fn is_redundant_nil_cond(pass: &Pass<'_>, cond: &Expr, ok: &Ident, assert_expr: &Expr) -> bool {
    let Expr::Ident(asserted) = assert_expr else {
        return false;
    };
    let Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) = cond else {
        return false;
    };
    if *op != Token::LAND {
        return false;
    }
    let assert_neq_nil = |e: &Expr| {
        let Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) = e else {
            return false;
        };
        *op == Token::NEQ
            && matches!(&**x, Expr::Ident(lhs) if lhs.name == asserted.name)
            && is_nil(pass, y)
    };
    match (&**x, &**y) {
        (Expr::Ident(id), rhs) if id.name == ok.name => assert_neq_nil(rhs),
        (lhs, Expr::Ident(id)) if id.name == ok.name => assert_neq_nil(lhs),
        _ => false,
    }
}

fn check_if(pass: &Pass<'_>, ifs: &IfStmt) -> bool {
    let init = ifs.init.as_deref().and_then(type_assert_init);
    let Some((ok, ta)) = init else {
        return false;
    };
    is_redundant_nil_cond(pass, &ifs.cond, ok, &ta.x)
}

fn check_nested_if(pass: &Pass<'_>, ifs: &IfStmt) -> bool {
    if ifs.init.is_some() {
        return false;
    }
    let Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) = &ifs.cond else {
        return false;
    };
    if *op != Token::NEQ || !is_nil(pass, y) {
        return false;
    };
    let Expr::Ident(lhs) = &**x else {
        return false;
    };
    if ifs.body.list.len() != 1 {
        return false;
    };
    let Stmt::IfStmt(inner) = &ifs.body.list[0] else {
        return false;
    };
    let Some((ok, ta)) = inner.init.as_deref().and_then(type_assert_init) else {
        return false;
    };
    if ta.x.id() != lhs.id() && !matches!((&*ta.x, &**x), (Expr::Ident(a), Expr::Ident(b)) if a.name == b.name) {
        return false;
    }
    matches!(&inner.cond, Expr::Ident(id) if id.name == ok.name)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1020 requires inspect analyzer".to_string())?
        .clone();

    let msg = "when ok is true, the asserted value can't be nil";
    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(IfStmt), pass.files(), |node| {
        let NodeRef::IfStmt(ifs) = node else {
            return;
        };
        if check_if(pass, ifs) || check_nested_if(pass, ifs) {
            pending.push((match_pos(node), msg.into()));
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
