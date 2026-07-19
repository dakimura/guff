//! `unreachable-code` — detect statements after control-flow exits.

use guff::ast::{BlockStmt, BranchStmt, CallExpr, Expr, ReturnStmt, Stmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::unparen;

const MESSAGE: &str = "unreachable code after this statement";

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::BlockStmt(block)) = n else {
                return true;
            };
            check_block(block, &mut failures);
            true
        });
    }
    failures
}

fn check_block(block: &BlockStmt, failures: &mut Vec<Failure>) {
    if block.list.len() < 2 {
        return;
    }
    for (i, stmt) in block.list[..block.list.len() - 1].iter().enumerate() {
        let next = &block.list[i + 1];
        if matches!(next, Stmt::LabeledStmt(_)) {
            continue;
        }
        match stmt {
            Stmt::ReturnStmt(r) => {
                failures.push(failure_at(r.return_.0));
                return;
            }
            Stmt::BranchStmt(b) if b.tok != Token::FALLTHROUGH => {
                failures.push(failure_at(b.tok_pos.0));
                return;
            }
            Stmt::ExprStmt(e) if is_branching_call(&e.x) && !matches!(next, Stmt::ReturnStmt(_)) => {
                failures.push(failure_at(e.x.pos().0));
                return;
            }
            _ => {}
        }
    }
}

fn is_branching_call(expr: &Expr) -> bool {
    let Expr::CallExpr(CallExpr { fun, .. }) = unparen(expr) else {
        return false;
    };
    match unparen(fun) {
        Expr::SelectorExpr(sel) => {
            let pkg = match unparen(&sel.x) {
                Expr::Ident(id) => id.name.as_str(),
                _ => return false,
            };
            matches!(
                (pkg, sel.sel.name.as_str()),
                ("os", "Exit")
                    | ("log", "Fatal" | "Fatalf" | "Fatalln" | "Panic" | "Panicf" | "Panicln")
                    | ("t" | "b" | "f", "Fatal" | "Fatalf" | "FailNow")
            )
        }
        _ => false,
    }
}

fn failure_at(pos: i64) -> Failure {
    Failure::new("unreachable-code", pos as u32, MESSAGE)
}
