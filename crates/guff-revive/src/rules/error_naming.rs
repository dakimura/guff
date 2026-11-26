//! `error-naming` — error vars should be named `errFoo` / `ErrFoo`.

use guff::ast::{CallExpr, Decl, Expr, File, GenDecl, Spec, ValueSpec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_blank, is_pkg_dot_name};

pub struct Checker {
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        // Package-level vars only (mirrors revive / the previous file.decls walk).
        // Function-local `var` GenDecls must not be checked.
        let NodeRef::File(file) = n else {
            return;
        };
        check_file(file, &mut self.failures);
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

fn check_file(file: &File, failures: &mut Vec<Failure>) {
    for decl in &file.decls {
        let Decl::GenDecl(GenDecl {
            tok: Some(Token::VAR),
            specs,
            ..
        }) = decl
        else {
            continue;
        };
        for spec in specs {
            let Spec::ValueSpec(ValueSpec {
                names, values, ..
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
            if !is_pkg_dot_name(fun, "errors", "New") && !is_pkg_dot_name(fun, "fmt", "Errorf") {
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
                    ..Failure::default()
                });
            }
        }
    }
}
