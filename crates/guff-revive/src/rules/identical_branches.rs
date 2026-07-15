//! `identical-branches` — warn when both branches of an if/else are identical.

use guff::ast::{IfStmt, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::astfmt::block_fmt;
use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::IfStmt(if_stmt)) = n else {
                return true;
            };
            check_if(if_stmt, &mut failures);
            true
        });
    }
    failures
}

fn check_if(if_stmt: &IfStmt, failures: &mut Vec<Failure>) {
    let Some(else_stmt) = if_stmt.else_.as_deref() else {
        return;
    };
    let Stmt::BlockStmt(else_block) = else_stmt else {
        return;
    };
    if block_fmt(&if_stmt.body) == block_fmt(else_block) {
        failures.push(Failure {
            rule: "identical-branches",
            pos: if_stmt.if_.0 as u32,
            message: "both branches of the if are identical".into(),
        });
    }
}
