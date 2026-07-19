//! `argument-limit` — restrict maximum number of function parameters (default 8).

use guff::ast::{Decl, FuncDecl};
use guff_analysis::Pass;

use crate::failure::Failure;

const MAX_ARGUMENTS: usize = 8;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            let num_params = count_params(f);
            if num_params > MAX_ARGUMENTS {
                failures.push(Failure {
                    rule: "argument-limit",
                    pos: f.name.name_pos.0 as u32,
                    message: format!(
                        "maximum number of arguments per function exceeded; max {MAX_ARGUMENTS} but got {num_params}"
                    ),
            confidence: None,
        });
            }
        }
    }
    failures
}

fn count_params(f: &FuncDecl) -> usize {
    let Some(params) = &f.ty.params else {
        return 0;
    };
    params
        .list
        .iter()
        .map(|field| {
            if field.names.is_empty() {
                1
            } else {
                field.names.len()
            }
        })
        .sum()
}
