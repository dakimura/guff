//! `waitgroup-by-value` — warn when `sync.WaitGroup` is passed by value.

use guff::ast::{Decl, FuncDecl};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_pkg_dot_type;

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
    let Some(params) = &f.ty.params else {
        return;
    };
    for field in &params.list {
        let Some(ty) = &field.ty else {
            continue;
        };
        if is_pkg_dot_type(ty, "sync", "WaitGroup") {
            failures.push(Failure {
                rule: "waitgroup-by-value",
                pos: ty.pos().0 as u32,
                message: "sync.WaitGroup passed by value, the function will get a copy of the original one".into(),
            confidence: None,
        });
        }
    }
}
