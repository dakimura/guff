//! `range-val-address` — warn when taking the address of a range value (pre-Go 1.22).

use guff::ast::{AssignStmt, CallExpr, CompositeLit, Expr, Ident, IndexExpr, KeyValueExpr, RangeStmt, UnaryExpr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{type_string, unparen};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::FuncDecl(f) = decl else {
                continue;
            };
            let Some(body) = &f.body else {
                continue;
            };
            walk::inspect(NodeRef::BlockStmt(body), |n| {
                let Some(NodeRef::RangeStmt(range)) = n else {
                    return true;
                };
                let Some(value_expr) = range.value.as_ref() else {
                    return true;
                };
                let Expr::Ident(value) = unparen(value_expr) else {
                    return true;
                };
                let value_is_ptr = pass
                    .types_info()
                    .and_then(|info| info.types.get(&value.id()))
                    .map(|t| type_string(pass, t.typ).starts_with('*'))
                    .unwrap_or(false);
                inspect_range_body(&range.body, value, value_is_ptr, &mut failures);
                true
            });
        }
    }
    failures
}

fn inspect_range_body(
    body: &guff::ast::BlockStmt,
    value: &Ident,
    value_is_ptr: bool,
    failures: &mut Vec<Failure>,
) {
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        match n {
            Some(NodeRef::AssignStmt(assign)) => {
                for lhs in &assign.lhs {
                    if let Expr::IndexExpr(IndexExpr { index, .. }) = unparen(lhs) {
                        if is_address_of_range_value(index, value, value_is_ptr) {
                            failures.push(make_failure(value, index));
                        }
                    }
                }
                for rhs in &assign.rhs {
                    check_addr_expr(rhs, value, value_is_ptr, failures);
                }
            }
            _ => {}
        }
        true
    });
}

fn check_addr_expr(expr: &Expr, value: &Ident, value_is_ptr: bool, failures: &mut Vec<Failure>) {
    if is_address_of_range_value(expr, value, value_is_ptr) {
        failures.push(make_failure(value, expr));
        return;
    }
    match unparen(expr) {
        Expr::CallExpr(call) if is_append(call) => {
            for arg in &call.args {
                if let Expr::CompositeLit(comp) = unparen(arg) {
                    check_composite(comp, value, value_is_ptr, failures);
                } else if is_address_of_range_value(arg, value, value_is_ptr) {
                    failures.push(make_failure(value, arg));
                }
            }
        }
        Expr::CompositeLit(comp) => check_composite(comp, value, value_is_ptr, failures),
        _ => {}
    }
}

fn check_composite(
    comp: &CompositeLit,
    value: &Ident,
    value_is_ptr: bool,
    failures: &mut Vec<Failure>,
) {
    for el in &comp.elts {
        let Expr::KeyValueExpr(KeyValueExpr { value: v, .. }) = el else {
            continue;
        };
        if is_address_of_range_value(v, value, value_is_ptr) {
            failures.push(make_failure(value, v));
        }
    }
}

fn is_address_of_range_value(expr: &Expr, value: &Ident, value_is_ptr: bool) -> bool {
    let Expr::UnaryExpr(UnaryExpr { op, x, .. }) = unparen(expr) else {
        return false;
    };
    if *op != Token::AND {
        return false;
    }
    refers_to_range_value(x, value, value_is_ptr)
}

fn refers_to_range_value(expr: &Expr, value: &Ident, value_is_ptr: bool) -> bool {
    match unparen(expr) {
        Expr::Ident(Ident { name, .. }) => name == &value.name,
        Expr::SelectorExpr(sel) => {
            if value_is_ptr {
                return false;
            }
            matches!(unparen(&sel.x), Expr::Ident(Ident { name, .. }) if name == &value.name)
        }
        _ => false,
    }
}

fn is_append(call: &CallExpr) -> bool {
    matches!(unparen(&call.fun), Expr::Ident(Ident { name, .. }) if name == "append")
}

fn make_failure(value: &Ident, node: &Expr) -> Failure {
    Failure::new(
        "range-val-address",
        node.pos().0 as u32,
        format!(
            "suspicious assignment of '{}'. range-loop variables always have the same address",
            value.name
        ),
    )
}
