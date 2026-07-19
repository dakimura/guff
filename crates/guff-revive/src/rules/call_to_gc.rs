//! `call-to-gc` — warn on explicit calls to the garbage collector.

use guff::ast::CallExpr;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_pkg_dot_name;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::CallExpr(call)) = n else {
                return true;
            };
            if is_pkg_dot_name(&call.fun, "runtime", "GC") {
                failures.push(Failure {
                    rule: "call-to-gc",
                    pos: call.fun.pos().0 as u32,
                    message: "explicit call to the garbage collector".into(),
            confidence: None,
        });
            }
            true
        });
    }
    failures
}
