//! `use-errors-new` — suggest `errors.New` instead of `fmt.Errorf` without verbs.

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
            if is_pkg_dot_name(&call.fun, "fmt", "Errorf") && call.args.len() == 1 {
                failures.push(Failure {
                    rule: "use-errors-new",
                    pos: call.fun.pos().0 as u32,
                    message: "replace fmt.Errorf by errors.New".into(),
            confidence: None,
        });
            }
            true
        });
    }
    failures
}
