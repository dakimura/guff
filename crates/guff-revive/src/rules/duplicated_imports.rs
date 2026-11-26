//! `duplicated-imports` — warn when the same package is imported twice.

use std::collections::HashSet;

use guff::ast::File;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::import_spec_pos;

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
                // Upstream reports `Node: imp` — the whole ImportSpec, whose
                // Pos() is the alias when there is one. Only the *path* is
                // compared, so `import "os"` and `import osdup "os"` are a
                // duplicate pair; reporting at `imp.path` puts the column six
                // characters to the right of upstream's for the aliased half.
                pos: import_spec_pos(imp),
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
