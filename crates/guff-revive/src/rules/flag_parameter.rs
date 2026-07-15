//! `flag-parameter` — warn on boolean parameters used as control flags.

use guff::ast::{Decl, Expr, FuncDecl, Ident, IfStmt, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_bool_type_expr;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            let Some(body) = &f.body else {
                continue;
            };
            let bool_params = collect_bool_params(f);
            if bool_params.is_empty() {
                continue;
            }
            walk::inspect(NodeRef::BlockStmt(body), |n| {
                let Some(NodeRef::IfStmt(if_stmt)) = n else {
                    return true;
                };
                if let Some(name) = cond_uses_bool_param(&if_stmt.cond, &bool_params) {
                    failures.push(Failure {
                        rule: "flag-parameter",
                        pos: if_stmt.if_.0 as u32,
                        message: format!(
                            "parameter '{name}' seems to be a control flag, avoid control coupling"
                        ),
                    });
                }
                true
            });
        }
    }
    failures
}

fn collect_bool_params(f: &FuncDecl) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let Some(params) = &f.ty.params else {
        return out;
    };
    for field in &params.list {
        if !field.ty.as_ref().is_some_and(is_bool_type_expr) {
            continue;
        }
        for name in &field.names {
            out.insert(name.name.clone());
        }
    }
    out
}

fn cond_uses_bool_param(cond: &Expr, params: &std::collections::HashSet<String>) -> Option<String> {
    match cond {
        Expr::Ident(Ident { name, .. }) if params.contains(name) => Some(name.clone()),
        Expr::BinaryExpr(b) => {
            cond_uses_bool_param(&b.x, params).or_else(|| cond_uses_bool_param(&b.y, params))
        }
        Expr::UnaryExpr(u) => cond_uses_bool_param(&u.x, params),
        Expr::ParenExpr(p) => cond_uses_bool_param(&p.x, params),
        _ => None,
    }
}
