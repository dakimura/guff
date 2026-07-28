//! SA5003 — defers in infinite loops will never execute.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5003`.

use std::sync::OnceLock;

use guff::ast::{BranchStmt, DeferStmt, ForStmt, ReturnStmt, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn walk_block(stmts: &[Stmt], might_exit: &mut bool, defers: &mut Vec<u32>) {
    for stmt in stmts {
        match stmt {
            Stmt::ReturnStmt(ReturnStmt { return_, .. }) => {
                *might_exit = true;
                return;
            }
            Stmt::BranchStmt(BranchStmt { tok, .. }) if *tok == Token::BREAK => {
                *might_exit = true;
                return;
            }
            Stmt::DeferStmt(DeferStmt { defer_, .. }) => defers.push(defer_.0 as u32),
            Stmt::BlockStmt(b) => walk_block(&b.list, might_exit, defers),
            Stmt::IfStmt(i) => {
                walk_block(&i.body.list, might_exit, defers);
                if *might_exit {
                    return;
                }
                if let Some(else_) = &i.else_ {
                    walk_block(std::slice::from_ref(else_.as_ref()), might_exit, defers);
                }
            }
            Stmt::ForStmt(f) => walk_block(&f.body.list, might_exit, defers),
            Stmt::RangeStmt(r) => walk_block(&r.body.list, might_exit, defers),
            _ => {}
        }
        if *might_exit {
            return;
        }
    }
}

fn check_loop(loop_: &ForStmt, pending: &mut Vec<u32>) {
    if loop_.cond.is_some() {
        return;
    }
    let mut might_exit = false;
    let mut defers = Vec::new();
    walk_block(&loop_.body.list, &mut might_exit, &mut defers);
    if might_exit {
        return;
    }
    pending.extend(defers);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA5003 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(ForStmt), pass.files(), |n| {
        let NodeRef::ForStmt(loop_) = n else {
            return;
        };
        check_loop(loop_, &mut pending);
    });
    for pos in pending {
        pass.report_unless_generated(pos, "defers in this infinite loop will never run");
    }
    Ok(None)
}

fn sa5003_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5003",
        doc: "defers in infinite loops will never execute",
        url: "https://staticcheck.dev/docs/checks/#SA5003",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5003_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5003_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
