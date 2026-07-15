//! `context-as-argument` — `context.Context` should be the first parameter.

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
    let params = &params.list;
    if params.len() <= 1 {
        return;
    }
    let mut ctx_allowed = true;
    for field in params {
        let is_ctx = field
            .ty
            .as_ref()
            .is_some_and(|t| is_pkg_dot_type(t, "context", "Context"));
        if is_ctx && !ctx_allowed {
            failures.push(Failure {
                rule: "context-as-argument",
                pos: field.pos().0 as u32,
                message: "context.Context should be the first parameter of a function".into(),
            });
            break;
        }
        if let Some(ty) = &field.ty {
            let rendered = crate::util::expr_string(ty);
            if rendered != "context.Context" {
                ctx_allowed = false;
            }
        } else {
            ctx_allowed = false;
        }
    }
}
