//! `datarace` — spot goroutines capturing named returns or range variables.

use std::collections::HashSet;

use guff::ast::{Decl, Expr, Field, FuncDecl, FuncLit, GoStmt, Ident, RangeStmt, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{go_version_at_least, unparen};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let go122_for = go_version_at_least(pass, 1, 22);
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            check_func(pass, f, go122_for, &mut failures);
        }
    }
    failures
}

fn check_func(pass: &Pass<'_>, f: &FuncDecl, go122_for: bool, failures: &mut Vec<Failure>) {
    let Some(body) = &f.body else {
        return;
    };
    let return_ids = extract_return_names(f);
    let mut range_ids: HashSet<String> = HashSet::new();
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        match n {
            Some(NodeRef::RangeStmt(r)) => {
                let ids = range_var_names(r);
                for id in &ids {
                    range_ids.insert(id.clone());
                }
                walk::inspect(NodeRef::BlockStmt(&r.body), |inner| {
                    if let Some(NodeRef::GoStmt(go)) = inner {
                        check_go_stmt(go, &return_ids, &range_ids, go122_for, failures);
                    }
                    true
                });
                for id in ids {
                    range_ids.remove(&id);
                }
                false
            }
            Some(NodeRef::GoStmt(go)) => {
                check_go_stmt(go, &return_ids, &range_ids, go122_for, failures);
                true
            }
            _ => true,
        }
    });
    let _ = pass;
}

fn extract_return_names(f: &FuncDecl) -> HashSet<String> {
    let mut out = HashSet::new();
    let Some(results) = &f.ty.results else {
        return out;
    };
    for field in &results.list {
        for name in &field.names {
            if name.name != "_" {
                out.insert(name.name.clone());
            }
        }
    }
    out
}

fn range_var_names(r: &RangeStmt) -> Vec<String> {
    let mut out = Vec::new();
    for expr in [&r.key, &r.value] {
        let Some(expr) = expr.as_ref() else {
            continue;
        };
        if let Expr::Ident(Ident { name, .. }) = unparen(expr) {
            if name != "_" {
                out.push(name.clone());
            }
        }
    }
    out
}

fn check_go_stmt(
    go: &GoStmt,
    return_ids: &HashSet<String>,
    range_ids: &HashSet<String>,
    go122_for: bool,
    failures: &mut Vec<Failure>,
) {
    let Expr::FuncLit(FuncLit { body, .. }) = unparen(&go.call.fun) else {
        return;
    };
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(NodeRef::Ident(Ident { name, name_pos, .. })) = n else {
            return true;
        };
        if name == "_" {
            return true;
        }
        if !go122_for && range_ids.contains(name) {
            failures.push(Failure {
                rule: "datarace",
                pos: name_pos.0 as u32,
                message: format!("datarace: range value {name} is captured (by-reference) in goroutine"),
            });
            return false;
        }
        if return_ids.contains(name) {
            failures.push(Failure {
                rule: "datarace",
                pos: name_pos.0 as u32,
                message: format!(
                    "potential datarace: return value {name} is captured (by-reference) in goroutine"
                ),
            });
            return false;
        }
        true
    });
}
