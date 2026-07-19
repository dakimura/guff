//! `redundant-import-alias` — warn when an import alias matches the package name.

use guff::ast::{Decl, Spec};
use guff_analysis::Pass;

use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
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
                let Some(alias) = &imp.name else {
                    continue;
                };
                let pkg_name = import_package_name(&imp.path.value);
                if alias.name == pkg_name {
                    failures.push(Failure {
                        rule: "redundant-import-alias",
                        pos: imp.path.pos().0 as u32,
                        message: format!("Import alias {:?} is redundant", alias.name),
            confidence: None,
        });
                }
            }
        }
    }
    failures
}

fn import_package_name(path: &str) -> String {
    let trimmed = path.trim_matches('"');
    trimmed
        .rsplit('/')
        .next()
        .unwrap_or(trimmed)
        .to_string()
}
