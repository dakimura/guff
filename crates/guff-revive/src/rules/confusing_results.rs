//! `confusing-results` — warn on consecutive unnamed results of the same type.

use guff::ast::Decl;
use guff_analysis::Pass;

use crate::astfmt::expr_fmt;
use crate::failure::Failure;

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
            if results.list.len() <= 1 {
                continue;
            }
            if results.list[0].names.first().is_some() {
                continue;
            }
            let mut last_type = String::new();
            for field in &results.list {
                let Some(ty_expr) = field.ty.as_ref() else {
                    continue;
                };
                let ty = expr_fmt(ty_expr);
                if ty == last_type {
                    failures.push(Failure {
                        rule: "confusing-results",
                        pos: field.pos().0 as u32,
                        message: "unnamed results of the same type may be confusing, consider using named results".into(),
                    });
                    break;
                }
                last_type = ty;
            }
        }
    }
    failures
}
