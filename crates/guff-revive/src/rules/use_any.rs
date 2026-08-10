//! `use-any` — suggest `any` instead of empty `interface{}`.

use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

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
                    let NodeRef::InterfaceType(it) = n else { return; };
                    if !it.methods.list.is_empty() {
                        return;
                    }
                    self.failures.push(Failure {
                        rule: "use-any",
                        pos: it.interface_.0 as u32,
                        message: "since Go 1.18 'interface{}' can be replaced by 'any'".into(),
                        ..Failure::default()
                    });
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

