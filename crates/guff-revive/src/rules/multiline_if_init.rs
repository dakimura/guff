//! `multiline-if-init` — warn when an `if` init clause spans multiple lines.

use guff::ast::IfStmt;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::line_of;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::IfStmt(if_stmt)) = n else {
                return true;
            };
            check_if_init(pass, if_stmt, &mut failures);
            true
        });
    }
    failures
}

fn check_if_init(pass: &Pass<'_>, if_stmt: &IfStmt, failures: &mut Vec<Failure>) {
    let Some(init) = &if_stmt.init else {
        return;
    };
    let start = line_of(pass, init.pos().0);
    let end = line_of(pass, init.end().0);
    if end > start {
        failures.push(Failure {
            rule: "multiline-if-init",
            pos: if_stmt.if_.0 as u32,
            message: "if-init statement should not span multiple lines".into(),
        });
    }
}
