//! `dot-imports` — forbid dot imports.

use guff::ast::{Decl, File, Spec};
use guff_analysis::Pass;

use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        check_file(file, &mut failures);
    }
    failures
}

fn check_file(file: &File, failures: &mut Vec<Failure>) {
    for decl in &file.decls {
        let Decl::GenDecl(g) = decl else {
            continue;
        };
        for spec in &g.specs {
            let Spec::ImportSpec(imp) = spec else {
                continue;
            };
            let is_dot = imp
                .name
                .as_ref()
                .is_some_and(|n| n.name == ".");
            if is_dot {
                failures.push(Failure {
                    rule: "dot-imports",
                    pos: imp.path.pos().0 as u32,
                    message: "should not use dot imports".into(),
                });
            }
        }
    }
}
