//! SA4018 — self-assignment of variables
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4018`.

use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};


use guff::ast::Expr;
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;

use crate::render::render_expr;

fn may_have_side_effects(expr: &Expr) -> bool {
    matches!(expr, Expr::CallExpr(_) | Expr::UnaryExpr(_) | Expr::BinaryExpr(_))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4018 requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(AssignStmt), pass.files(), |node| {
        let NodeRef::AssignStmt(assign) = node else { return };
        if assign.tok != Some(Token::ASSIGN) || assign.lhs.len() != assign.rhs.len() { return; }
        for (lhs, rhs) in assign.lhs.iter().zip(assign.rhs.iter()) {
            if may_have_side_effects(lhs) || may_have_side_effects(rhs) { continue; }
            if std::mem::discriminant(lhs) != std::mem::discriminant(rhs) { continue; }
            let rl = render_expr(lhs);
            let rr = render_expr(rhs);
            if rl == rr {
                // Upstream reports the AssignStmt node — the start of its
                // first LHS operand, not the `=`.
                pending.push((match_pos(node), format!("self-assignment of {rr} to {rl}")));
            }
        }
    });
    for (pos, msg) in pending { pass.report_unless_generated(pos, msg); }
    Ok(None)
}


fn sa4018_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4018",
        doc: "self-assignment of variables",
        url: "https://staticcheck.dev/docs/checks/#SA4018",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4018_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4018_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
