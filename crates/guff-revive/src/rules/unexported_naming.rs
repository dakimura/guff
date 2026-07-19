//! `unexported-naming` — warn when local symbols use exported (uppercase) names.

use guff::ast::{AssignStmt, Decl, Expr, Ident};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_exported_ident, unparen};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            lint_fields(&f.ty.params, &mut failures);
            lint_fields(&f.ty.results, &mut failures);
        }
        walk::inspect(NodeRef::File(file), |n| {
            match n {
                Some(NodeRef::FuncLit(f)) => {
                    lint_fields(&f.ty.params, &mut failures);
                    lint_fields(&f.ty.results, &mut failures);
                }
                Some(NodeRef::AssignStmt(a)) => lint_assign(a, &mut failures),
                Some(NodeRef::ValueSpec(vs)) => {
                    for id in &vs.names {
                        lint_ident(id, &mut failures);
                    }
                }
                _ => {}
            }
            true
        });
    }
    failures
}

fn lint_assign(a: &AssignStmt, failures: &mut Vec<Failure>) {
    if a.tok != Some(Token::DEFINE) {
        return;
    }
    for lhs in &a.lhs {
        if let Expr::Ident(id) = unparen(lhs) {
            lint_ident(id, failures);
        }
    }
}

fn lint_fields(fields: &Option<guff::ast::FieldList>, failures: &mut Vec<Failure>) {
    let Some(fields) = fields else {
        return;
    };
    for field in &fields.list {
        for id in &field.names {
            lint_ident(id, failures);
        }
    }
}

fn lint_ident(id: &Ident, failures: &mut Vec<Failure>) {
    if is_exported_ident(&id.name) {
        failures.push(Failure {
            rule: "unexported-naming",
            pos: id.name_pos.0 as u32,
            message: format!(
                "the symbol {} is local, its name should start with a lowercase letter",
                id.name
            ),
            confidence: None,
        });
    }
}
