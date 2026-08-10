//! `use-errors-new` — suggest `errors.New` instead of `fmt.Errorf` without verbs.

use guff::ast::CallExpr;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_pkg_dot_name;

pub struct Checker {
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
                    let NodeRef::CallExpr(call) = n else { return; };
                    if is_pkg_dot_name(&call.fun, "fmt", "Errorf") && call.args.len() == 1 {
                        self.failures.push(Failure {
                            rule: "use-errors-new",
                            pos: call.fun.pos().0 as u32,
                            message: "replace fmt.Errorf by errors.New".into(),
                            ..Failure::default()
                        });
                    }
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

