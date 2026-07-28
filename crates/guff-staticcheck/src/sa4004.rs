//! SA4004 — loop exits unconditionally after one iteration.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4004`.

use std::sync::OnceLock;

use guff::ast::{BranchStmt, ReturnStmt, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4004 requires inspect analyzer".to_string())?;
    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(FuncDecl, FuncLit), pass.files(), |node| {
        let body = match node {
            NodeRef::FuncDecl(f) => f.body.as_ref().map(|b| &b.list),
            NodeRef::FuncLit(f) => Some(&f.body.list),
            _ => None,
        };
        let Some(body) = body else {
            return;
        };
        for stmt in body {
            let loop_body = match stmt {
                Stmt::ForStmt(f) => &f.body.list,
                Stmt::RangeStmt(r) => &r.body.list,
                _ => continue,
            };
            if loop_body.len() < 2 {
                continue;
            }
            let mut unconditional: Option<u32> = None;
            let mut has_branching = false;
            let mut give_up = false;
            for s in loop_body {
                match s {
                    Stmt::BranchStmt(BranchStmt {
                        tok: Token::BREAK,
                        label: None,
                        tok_pos,
                        ..
                    }) => unconditional = Some(tok_pos.0 as u32),
                    Stmt::BranchStmt(BranchStmt {
                        tok: Token::CONTINUE,
                        label: None,
                        ..
                    }) => {
                        // Top-level continue: loop is not unconditionally terminated.
                        give_up = true;
                        break;
                    }
                    Stmt::ReturnStmt(ReturnStmt { return_, .. }) => {
                        unconditional = Some(return_.0 as u32)
                    }
                    Stmt::IfStmt(_)
                    | Stmt::ForStmt(_)
                    | Stmt::RangeStmt(_)
                    | Stmt::SwitchStmt(_)
                    | Stmt::SelectStmt(_) => {
                        has_branching = true;
                    }
                    _ => {}
                }
            }
            if give_up || unconditional.is_none() || !has_branching {
                continue;
            }
            // Upstream second pass: nested goto / continue cancels the finding.
            let mut nested_cancels = false;
            for s in loop_body {
                walk::preorder(walk::stmt_ref(s), |n| {
                    if nested_cancels {
                        return false;
                    }
                    if let NodeRef::BranchStmt(b) = n {
                        match b.tok {
                            Token::GOTO => nested_cancels = true,
                            Token::CONTINUE if b.label.is_none() => nested_cancels = true,
                            _ => {}
                        }
                    }
                    true
                });
            }
            if nested_cancels {
                continue;
            }
            if let Some(pos) = unconditional {
                pending.push((
                    pos,
                    "the surrounding loop is unconditionally terminated".into(),
                ));
            }
        }
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4004_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4004",
        doc: "the loop exits unconditionally after one iteration",
        url: "https://staticcheck.dev/docs/checks/#SA4004",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4004_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4004_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
