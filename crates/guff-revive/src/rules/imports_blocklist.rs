//! `imports-blocklist` — warn when importing block-listed packages.

use guff::ast::{Decl, Spec};
use guff_analysis::Pass;
use regex::Regex;

use crate::config;
use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let entries = config::imports_blocklist_entries();
    if entries.is_empty() {
        return Vec::new();
    }
    let patterns: Vec<Regex> = entries
        .iter()
        .filter_map(|entry| compile_blocklist_pattern(entry).ok())
        .collect();
    if patterns.is_empty() {
        return Vec::new();
    }

    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::GenDecl(g) = decl else {
                continue;
            };
            for spec in &g.specs {
                let Spec::ImportSpec(imp) = spec else {
                    continue;
                };
                let path = imp.path.value.as_str();
                if patterns.iter().any(|re| re.is_match(path)) {
                    failures.push(Failure {
                        rule: "imports-blocklist",
                        pos: imp.path.pos().0 as u32,
                        message: format!(
                            "should not use the following blocklisted import: {path}"
                        ),
                    });
                }
            }
        }
    }
    failures
}

fn compile_blocklist_pattern(entry: &str) -> Result<Regex, regex::Error> {
    let glob = entry.replace("/**/", "(\\W|\\w)*");
    Regex::new(&format!(r#"(?m)"{glob}"$"#))
}
