//! `redundant-import-alias` — warn when an import alias matches the package name.

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
        let NodeRef::ImportSpec(imp) = n else {
            return;
        };
        let Some(alias) = &imp.name else {
            return;
        };
        let pkg_name = import_package_name(&imp.path.value);
        if alias.name == pkg_name {
            self.failures.push(Failure {
                rule: "redundant-import-alias",
                pos: imp.path.pos().0 as u32,
                message: format!("Import alias {:?} is redundant", alias.name),
                confidence: None,
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

fn import_package_name(path: &str) -> String {
    let trimmed = path.trim_matches('"');
    trimmed
        .rsplit('/')
        .next()
        .unwrap_or(trimmed)
        .to_string()
}
