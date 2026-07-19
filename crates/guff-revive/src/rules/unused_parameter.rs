//! `unused-parameter` — warn on unused function parameters.

use guff::ast::{Decl, Expr, FuncDecl, FuncLit, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_blank;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            if let Some(body) = &f.body {
                if let Some(params) = &f.ty.params {
                    check_func(&params.list, body, &mut failures);
                }
            }
        }
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::FuncLit(f)) = n else {
                return true;
            };
            if let Some(params) = &f.ty.params {
                check_func(&params.list, &f.body, &mut failures);
            }
            true
        });
    }
    failures
}

fn check_func(
    params: &[guff::ast::Field],
    body: &guff::ast::BlockStmt,
    failures: &mut Vec<Failure>,
) {
    let mut unused: Vec<(String, i64)> = Vec::new();
    for field in params {
        for name in &field.names {
            if is_blank(name) {
                continue;
            }
            unused.push((name.name.clone(), name.name_pos.0));
        }
    }
    if unused.is_empty() {
        return;
    }
    let mut used = std::collections::HashSet::new();
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::Ident(id) = n {
            used.insert(id.name.clone());
        }
        true
    });
    for (name, pos) in unused {
        if !used.contains(&name) {
            failures.push(Failure {
                rule: "unused-parameter",
                pos: pos as u32,
                message: format!(
                    "parameter '{name}' seems to be unused, consider removing or renaming it as _"
                ),
            confidence: None,
        });
        }
    }
}
