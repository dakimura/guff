//! `error-return` — `error` should be the last return type.

use guff::ast::{Decl, FuncDecl};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_error_ident_type;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            check_func(f, &mut failures);
        }
    }
    failures
}

fn check_func(f: &FuncDecl, failures: &mut Vec<Failure>) {
    let Some(results) = &f.ty.results else {
        return;
    };
    if results.list.len() <= 1 {
        return;
    }
    let last = results.list.last().expect("checked len");
    if last.ty.as_ref().is_some_and(|t| is_error_ident_type(t)) {
        return;
    }
    for r in &results.list[..results.list.len() - 1] {
        if r.ty.as_ref().is_some_and(|t| is_error_ident_type(t)) {
            failures.push(Failure {
                rule: "error-return",
                pos: f.name.name_pos.0 as u32,
                message: "error should be the last type when returning multiple items".into(),
            });
            break;
        }
    }
}
