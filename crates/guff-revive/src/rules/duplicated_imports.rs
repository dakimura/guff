//! `duplicated-imports` — warn when the same package is imported twice.

use std::collections::HashSet;

use guff::ast::File;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

pub struct Checker {
    seen: HashSet<String>,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            seen: HashSet::new(),
            failures: Vec::new(),
        }
    }

    pub fn on_file(&mut self, _file: &File) {
        self.seen.clear();
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::ImportSpec(imp) = n else {
            return;
        };
        let path = imp.path.value.clone();
        if !self.seen.insert(path.clone()) {
            self.failures.push(Failure {
                rule: "duplicated-imports",
                pos: imp.path.pos().0 as u32,
                message: format!("Package {path} already imported"),
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
        c.on_file(file);
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}
