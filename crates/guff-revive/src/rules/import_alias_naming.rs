//! `import-alias-naming` — enforce conventions for import alias names.

use guff::walk::{self, NodeRef};
use guff_analysis::Pass;
use regex::Regex;
use std::sync::OnceLock;

use crate::failure::Failure;

fn default_allow() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-z][a-z0-9]{0,}$").expect("valid regex"))
}

pub struct Checker {
    allow: &'static Regex,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            allow: default_allow(),
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
        if alias.name == "_" || alias.name == "." {
            return;
        }
        if !self.allow.is_match(&alias.name) {
            self.failures.push(Failure {
                rule: "import-alias-naming",
                pos: alias.name_pos.0 as u32,
                message: format!(
                    "import name ({}) must match the regular expression: {}",
                    alias.name,
                    self.allow.as_str()
                ),
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
