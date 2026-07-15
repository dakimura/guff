//! `function-result-limit` — restrict maximum number of return results (default 3).

use guff::ast::Decl;
use guff_analysis::Pass;

use crate::failure::Failure;

const MAX_RESULTS: usize = 3;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            let Some(results) = &f.ty.results else {
                continue;
            };
            let num: usize = results
                .list
                .iter()
                .map(|field| {
                    if field.names.is_empty() {
                        1
                    } else {
                        field.names.len()
                    }
                })
                .sum();
            if num > MAX_RESULTS {
                failures.push(Failure {
                    rule: "function-result-limit",
                    pos: f.name.name_pos.0 as u32,
                    message: format!(
                        "maximum number of return results per function exceeded; max {MAX_RESULTS} but got {num}"
                    ),
                });
            }
        }
    }
    failures
}
