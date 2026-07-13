//! SA3001 — assigning to `b.N` in benchmarks.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa3001`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, SelectorExpr};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::is_of_type_with_name;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::render::render_expr;

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA3001 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::AssignStmt(AssignStmt { tok, lhs, .. }) = node else {
            return;
        };
        if *tok != Some(Token::ASSIGN) || lhs.len() != 1 {
            return;
        }
        let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = &lhs[0] else {
            return;
        };
        if sel.name != "N" {
            return;
        }
        if !is_of_type_with_name(pass, x, "*testing.B") {
            return;
        }
        pending.push((
            sel.name_pos.0 as u32,
            format!("should not assign to {}", render_expr(&lhs[0])),
        ));
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn sa3001_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA3001",
        doc: "assigning to b.N in benchmarks",
        url: "https://staticcheck.dev/docs/checks/#SA3001",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA3001 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa3001_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa3001_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
