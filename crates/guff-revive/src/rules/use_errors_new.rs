//! `use-errors-new` — suggest `errors.New` instead of `fmt.Errorf` without verbs.

use guff::ast::CallExpr;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::go_version_at_least;
use crate::util::is_pkg_dot_name;

pub struct Checker {
    failures: Vec<Failure>,
}

impl Checker {
    pub fn try_new(pass: &Pass<'_>) -> Option<Self> {
        // revive v1.17.0: "for unformatted strings in Go 1.26, fmt.Errorf matches
        // the behavior of errors.New".
        if go_version_at_least(pass, 1, 26) {
            return None;
        }
        Some(Self::new())
    }

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
    let Some(mut c) = Checker::try_new(pass) else {
        return Vec::new();
    };
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

