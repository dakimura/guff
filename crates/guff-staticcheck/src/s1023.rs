//! S1023 — omit redundant control flow.
//!
//! Port of `honnef.co/go/tools/simple/s1023`.

use std::sync::OnceLock;

use guff::ast::{CaseClause, FuncType, Stmt};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn func_has_results(ty: &FuncType) -> bool {
    ty.results
        .as_ref()
        .is_some_and(|fl| !fl.list.is_empty())
}

fn check_case_clause(clause: &CaseClause) -> Option<u32> {
    if clause.body.len() < 2 {
        return None;
    }
    let Stmt::BranchStmt(branch) = clause.body.last()? else {
        return None;
    };
    if branch.tok != Token::BREAK || branch.label.is_some() {
        return None;
    }
    Some(branch.tok_pos.0 as u32)
}

fn check_func_body(ty: &FuncType, body: &guff::ast::BlockStmt) -> Option<u32> {
    if func_has_results(ty) || body.list.is_empty() {
        return None;
    }
    let Stmt::ReturnStmt(ret) = body.list.last()? else {
        return None;
    };
    if !ret.results.is_empty() {
        return None;
    }
    Some(ret.return_.0 as u32)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1023 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |n| {
        match n {
            NodeRef::CaseClause(clause) => {
                if let Some(pos) = check_case_clause(clause) {
                    pending.push((pos, "redundant break statement".into()));
                }
            }
            NodeRef::FuncDecl(func) => {
                if let Some(body) = &func.body {
                    if let Some(pos) = check_func_body(&func.ty, body) {
                        pending.push((pos, "redundant return statement".into()));
                    }
                }
            }
            NodeRef::FuncLit(func) => {
                if let Some(pos) = check_func_body(&func.ty, &func.body) {
                    pending.push((pos, "redundant return statement".into()));
                }
            }
            _ => {}
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1023_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1023",
        doc: "omit redundant control flow",
        url: "https://staticcheck.dev/docs/checks/#S1023",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1023 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1023_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1023_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
