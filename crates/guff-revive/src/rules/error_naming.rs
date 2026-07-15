//! `error-naming` — error vars should be named `errFoo` / `ErrFoo`.

use guff::ast::{CallExpr, Decl, Expr, GenDecl, Spec, ValueSpec};
use guff::token::Token;
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_blank, is_pkg_dot_name};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::GenDecl(GenDecl { tok: Some(Token::VAR), specs, .. }) = decl else {
                continue;
            };
            for spec in specs {
                let Spec::ValueSpec(ValueSpec {
                    names,
                    values,
                    ..
                }) = spec
                else {
                    continue;
                };
                if names.len() != 1 || values.len() != 1 {
                    continue;
                }
                let Expr::CallExpr(CallExpr { fun, .. }) = &values[0] else {
                    continue;
                };
                if !is_pkg_dot_name(fun, "errors", "New") && !is_pkg_dot_name(fun, "fmt", "Errorf")
                {
                    continue;
                }
                let id = &names[0];
                if is_blank(id) {
                    continue;
                }
                let prefix = if id.is_exported() { "Err" } else { "err" };
                if !id.name.starts_with(prefix) {
                    failures.push(Failure {
                        rule: "error-naming",
                        pos: id.name_pos.0 as u32,
                        message: format!(
                            "error var {} should have name of the form {}Foo",
                            id.name, prefix
                        ),
                    });
                }
            }
        }
    }
    failures
}
