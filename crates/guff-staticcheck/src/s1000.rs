//! S1000 — use plain channel send or receive instead of single-case select.
//!
//! Port of `honnef.co/go/tools/simple/s1000`.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, ForStmt, SelectStmt, Stmt, UnaryExpr};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn comm_clauses<'a>(sel: &'a SelectStmt) -> Vec<&'a guff::ast::CommClause> {
    sel.body
        .list
        .iter()
        .filter_map(|s| match s {
            Stmt::CommClause(c) => Some(c),
            _ => None,
        })
        .collect()
}

fn is_recv_comm(comm: &Stmt) -> bool {
    match comm {
        Stmt::ExprStmt(e) => matches!(
            &e.x,
            Expr::UnaryExpr(UnaryExpr { op: Token::ARROW, .. })
        ),
        Stmt::AssignStmt(AssignStmt { rhs, .. }) => rhs.first().is_some_and(|e| {
            matches!(e, Expr::UnaryExpr(UnaryExpr { op: Token::ARROW, .. }))
        }),
        _ => false,
    }
}

fn check_for_select(fs: &ForStmt) -> bool {
    if fs.init.is_some() || fs.cond.is_some() || fs.post.is_some() {
        return false;
    }
    if fs.body.list.len() != 1 {
        return false;
    }
    let Stmt::SelectStmt(sel) = &fs.body.list[0] else {
        return false;
    };
    let clauses = comm_clauses(sel);
    clauses.len() == 1 && clauses[0].comm.as_deref().is_some_and(is_recv_comm)
}

fn check_single_select(sel: &SelectStmt) -> bool {
    comm_clauses(sel).len() == 1
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1000 requires inspect analyzer".to_string())?
        .clone();

    let mut seen_selects = HashSet::new();
    let mut pending: Vec<(u32, String)> = Vec::new();

    inspect.preorder_typed(node_mask!(ForStmt, SelectStmt), pass.files(), |node| {
        if let NodeRef::ForStmt(fs) = node {
            if check_for_select(fs) {
                if let Stmt::SelectStmt(sel) = &fs.body.list[0] {
                    seen_selects.insert(sel as *const _ as usize);
                }
                pending.push((
                    match_pos(node),
                    "should use for range instead of for { select {} }".into(),
                ));
            }
        }
        if let NodeRef::SelectStmt(sel) = node {
            if check_single_select(sel) && !seen_selects.contains(&(sel as *const _ as usize)) {
                pending.push((
                    match_pos(node),
                    "should use a simple channel send/receive instead of select with a single case"
                        .into(),
                ));
            }
        }
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1000_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1000",
        doc: "use plain channel send or receive instead of single-case select",
        url: "https://staticcheck.dev/docs/checks/#S1000",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1000_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1000_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
