//! `modifies-parameter` — warn on assignments to function parameters.

use guff::ast::{CallExpr, Decl, Expr, FuncDecl, Ident, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::astfmt::expr_fmt;
use crate::failure::Failure;
use crate::util::unparen;

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
            let params = collect_param_names(f);
            if params.is_empty() {
                continue;
            }
            walk::inspect(NodeRef::BlockStmt(body), |n| {
                match n {
                    Some(NodeRef::IncDecStmt(inc)) => {
                        if let Expr::Ident(id) = unparen(&inc.x) {
                            check_param(id, &params, &mut failures);
                        }
                    }
                    Some(NodeRef::AssignStmt(assign)) => {
                        for (i, lhs) in assign.lhs.iter().enumerate() {
                            if let Expr::Ident(id) = unparen(lhs) {
                                if i < assign.rhs.len() {
                                    check_modifying_call(&assign.rhs[i], &params, &mut failures);
                                }
                                check_param(id, &params, &mut failures);
                            }
                        }
                    }
                    Some(NodeRef::ExprStmt(expr)) => {
                        if let Expr::CallExpr(call) = &expr.x {
                            check_modifying_call(&Expr::CallExpr(call.clone()), &params, &mut failures);
                        }
                    }
                    _ => {}
                }
                true
            });
        }
    }
    failures
}

fn collect_param_names(f: &FuncDecl) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let Some(params) = &f.ty.params else {
        return out;
    };
    for field in &params.list {
        for name in &field.names {
            if name.name != "_" {
                out.insert(name.name.clone());
            }
        }
    }
    out
}

fn check_param(id: &Ident, params: &std::collections::HashSet<String>, failures: &mut Vec<Failure>) {
    if params.contains(&id.name) {
        failures.push(Failure {
            rule: "modifies-parameter",
            pos: id.name_pos.0 as u32,
            message: format!("parameter '{}' seems to be modified", id.name),
            confidence: None,
        });
    }
}

fn check_modifying_call(
    node: &Expr,
    params: &std::collections::HashSet<String>,
    failures: &mut Vec<Failure>,
) {
    let Expr::CallExpr(call) = node else {
        return;
    };
    let func_name = expr_fmt(&call.fun);
    let modifying = match func_name.as_str() {
        "slices.Delete" | "slices.DeleteFunc" => Some(0),
        _ => None,
    };
    let Some(pos) = modifying else {
        return;
    };
    let Some(arg) = call.args.get(pos) else {
        return;
    };
    let Expr::Ident(id) = unparen(arg) else {
        return;
    };
    if params.contains(&id.name) {
        failures.push(Failure {
            rule: "modifies-parameter",
            pos: call.fun.pos().0 as u32,
            message: format!(
                "parameter '{}' seems to be modified by '{}'",
                id.name, func_name
            ),
            confidence: None,
        });
    }
}
