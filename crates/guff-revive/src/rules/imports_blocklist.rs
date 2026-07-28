//! `imports-blocklist` — warn when importing block-listed packages.

use guff::walk::{self, NodeRef};
use guff_analysis::Pass;
use regex::Regex;

use crate::config;
use crate::failure::Failure;

pub struct Checker {
    patterns: Vec<Regex>,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn try_new(pass: &Pass<'_>) -> Option<Self> {
        let entries = config::imports_blocklist_entries(pass);
        if entries.is_empty() {
            return None;
        }
        let patterns: Vec<Regex> = entries
            .iter()
            .filter_map(|entry| compile_blocklist_pattern(entry).ok())
            .collect();
        if patterns.is_empty() {
            return None;
        }
        Some(Self {
            patterns,
            failures: Vec::new(),
        })
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::ImportSpec(imp) = n else {
            return;
        };
        let path = imp.path.value.as_str();
        if self.patterns.iter().any(|re| re.is_match(path)) {
            self.failures.push(Failure {
                rule: "imports-blocklist",
                pos: imp.path.pos().0 as u32,
                message: format!("should not use the following blocklisted import: {path}"),
                confidence: None,
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

fn compile_blocklist_pattern(entry: &str) -> Result<Regex, regex::Error> {
    let glob = entry.replace("/**/", "(\\W|\\w)*");
    Regex::new(&format!(r#"(?m)"{glob}"$"#))
}
