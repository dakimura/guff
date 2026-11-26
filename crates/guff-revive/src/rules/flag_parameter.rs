//! `flag-parameter` — warn on boolean parameters used as control flags.

use std::collections::HashSet;

use guff::ast::{Expr, FuncDecl, Ident};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_bool_type_expr;

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
        let NodeRef::FuncDecl(f) = n else {
            return;
        };
        let Some(body) = &f.body else {
            return;
        };
        let params_pos = f.ty.params.as_ref().map(|p| p.opening.0 as u32);
        let bool_params = collect_bool_params(f);
        if bool_params.is_empty() {
            return;
        }
        walk::inspect(NodeRef::BlockStmt(body), |n| {
            let Some(NodeRef::IfStmt(if_stmt)) = n else {
                return true;
            };
            if let Some(name) = cond_uses_bool_param(&if_stmt.cond, &bool_params) {
                // Upstream reports the function's *parameter list*, whose
                // Pos() is the opening parenthesis — not the flag parameter
                // itself and not the `if` that reads it.
                let pos = params_pos.unwrap_or(if_stmt.if_.0 as u32);
                self.failures.push(Failure {
                    rule: "flag-parameter",
                    pos,
                    message: format!(
                        "parameter '{name}' seems to be a control flag, avoid control coupling"
                    ),
                    ..Failure::default()
                });
            }
            true
        });
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

fn collect_bool_params(f: &FuncDecl) -> HashSet<String> {
    let mut out = HashSet::new();
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

fn cond_uses_bool_param(cond: &Expr, params: &HashSet<String>) -> Option<String> {
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
