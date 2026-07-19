//! `duplicated-imports` — warn when the same package is imported twice.

use guff::ast::{Decl, Spec};
use guff_analysis::Pass;

use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        let mut seen = std::collections::HashSet::new();
        for decl in &file.decls {
            let Decl::GenDecl(g) = decl else {
                continue;
            };
            for spec in &g.specs {
                let Spec::ImportSpec(imp) = spec else {
                    continue;
                };
                let path = imp.path.value.clone();
                if !seen.insert(path.clone()) {
                    failures.push(Failure {
                        rule: "duplicated-imports",
                        pos: imp.path.pos().0 as u32,
                        message: format!("Package {path} already imported"),
            confidence: None,
        });
                }
            }
        }
    }
    failures
}
