//! ST1015 — a switch's default case should be the first or last case.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1015`.

use std::sync::OnceLock;

use guff::ast::{CaseClause, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn has_fallthrough(clause: &CaseClause) -> bool {
    for stmt in clause.body.iter().rev() {
        match stmt {
            Stmt::EmptyStmt(_) => {}
            Stmt::BranchStmt(b) => return b.tok == Token::FALLTHROUGH,
            _ => return false,
        }
    }
    false
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1015 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(SwitchStmt), pass.files(), |node| {
        let NodeRef::SwitchStmt(stmt) = node else {
            return;
        };
        let list = &stmt.body.list;
        let mut default_idx = None;
        for (i, c) in list.iter().enumerate() {
            let Stmt::CaseClause(cc) = c else {
                continue;
            };
            if cc.list.is_empty() {
                default_idx = Some(i);
                break;
            }
        }
        let Some(default_idx) = default_idx else {
            return;
        };
        if default_idx == 0 || default_idx == list.len() - 1 {
            return;
        }
        let Stmt::CaseClause(default_clause) = &list[default_idx] else {
            return;
        };
        let Stmt::CaseClause(prev) = &list[default_idx - 1] else {
            return;
        };
        if has_fallthrough(prev) || has_fallthrough(default_clause) {
            return;
        }
        pending.push((
            default_clause.case.0 as u32,
            "default case should be first or last in switch statement".into(),
        ));
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn st1015_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1015",
        doc: "a switch's default case should be the first or last case",
        url: "https://staticcheck.dev/docs/checks/#ST1015",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1015_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1015_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
