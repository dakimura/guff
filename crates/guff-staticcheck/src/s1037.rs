//! S1037 — use time.Sleep instead of select with time.After.
//!
//! Port of `honnef.co/go/tools/simple/s1037`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, Stmt, UnaryExpr};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, is_call_to, is_call_to_any};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1037 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::SelectStmt(sel) = node else {
            return;
        };
        let clauses: Vec<_> = sel
            .body
            .list
            .iter()
            .filter_map(|s| match s {
                Stmt::CommClause(c) => Some(c),
                _ => None,
            })
            .collect();
        if clauses.len() != 1 {
            return;
        }
        let Some(comm) = clauses[0].comm.as_deref() else {
            return;
        };
        let recv_expr = match comm {
            Stmt::ExprStmt(es) => match &es.x {
                Expr::UnaryExpr(UnaryExpr { op: Token::ARROW, x, .. }) => Some(&**x),
                _ => None,
            },
            Stmt::AssignStmt(AssignStmt { rhs, .. }) if rhs.len() == 1 => match &rhs[0] {
                Expr::UnaryExpr(UnaryExpr { op: Token::ARROW, x, .. }) => Some(&**x),
                _ => None,
            },
            _ => None,
        };
        let Some(call_expr) = recv_expr else {
            return;
        };
        let Expr::CallExpr(call) = call_expr else {
            return;
        };
        let is_after = is_call_to_any(pass, call, &["time.After"])
            || matches!(&*call.fun, Expr::SelectorExpr(sel) if sel.sel.name == "After");
        if !is_after {
            return;
        }
        pending.push((
            match_pos(node),
            "should use time.Sleep instead of elaborate way of sleeping".into(),
        ));
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1037_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1037",
        doc: "use time.Sleep instead of select with time.After",
        url: "https://staticcheck.dev/docs/checks/#S1037",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1037 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1037_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1037_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
