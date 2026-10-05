//! `use-any` — suggest `any` instead of empty `interface{}`.

use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::go_version_at_least;

pub struct Checker {
    failures: Vec<Failure>,
}

impl Checker {
    pub fn try_new(pass: &Pass<'_>) -> Option<Self> {
        // revive v1.17.0: the alias `any` exists from Go 1.18.
        if !go_version_at_least(pass, 1, 18) {
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

