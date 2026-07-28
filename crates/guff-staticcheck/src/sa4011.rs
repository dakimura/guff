//! SA4011 — break statement with no effect in switch/select.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4011`.

use std::sync::OnceLock;

use guff::ast::{BranchStmt, CaseClause, CommClause, IfStmt, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_breaks_in_block(body: &[Stmt], pending: &mut Vec<(u32, String)>) {
    for stmt in body {
        let blocks: Vec<&[Stmt]> = match stmt {
            Stmt::SwitchStmt(sw) => sw
                .body
                .list
                .iter()
                .filter_map(|c| {
                    if let Stmt::CaseClause(CaseClause { body, .. }) = c {
                        Some(body.as_slice())
                    } else {
                        None
                    }
                })
                .collect(),
            Stmt::SelectStmt(sel) => sel
                .body
                .list
                .iter()
                .filter_map(|c| {
                    if let Stmt::CommClause(CommClause { body, .. }) = c {
                        Some(body.as_slice())
                    } else {
                        None
                    }
                })
                .collect(),
            _ => continue,
        };
        for block in blocks {
            if block.is_empty() {
                continue;
            }
            let last_idx = block.len() - 1;
            let mut lasts: Vec<&Stmt> = vec![&block[last_idx]];
            if let Stmt::IfStmt(ifs) = &block[last_idx] {
                if let Some(l) = ifs.body.list.last() {
                    lasts[0] = l;
                }
                if let Some(else_stmt) = &ifs.else_ {
                    if let Stmt::BlockStmt(else_block) = &**else_stmt {
                        if let Some(l) = else_block.list.last() {
                            lasts.push(l);
                        }
                    }
                }
            }
            for last in lasts {
                if let Stmt::BranchStmt(BranchStmt {
                    tok: Token::BREAK,
                    label: None,
                    tok_pos,
                    ..
                }) = last
                {
                    pending.push((
                        tok_pos.0 as u32,
                        "ineffective break statement. Did you mean to break out of the outer loop?"
                            .into(),
                    ));
                }
            }
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4011 requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(ForStmt, RangeStmt), pass.files(), |node| {
        let body = match node {
            NodeRef::ForStmt(f) => &f.body.list,
            NodeRef::RangeStmt(r) => &r.body.list,
            _ => return,
        };
        check_breaks_in_block(body, &mut pending);
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4011_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4011",
        doc: "break statement with no effect in switch/select",
        url: "https://staticcheck.dev/docs/checks/#SA4011",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4011_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4011_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
