//! `range-val-address` — warn when taking the address of a range value (pre-Go 1.22).

use guff::ast::{
    CallExpr, CompositeLit, Expr, Ident, IndexExpr, KeyValueExpr, UnaryExpr,
};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{go_version_at_least, type_string, unparen};

/// Upstream returns early for Go 1.22+ packages: from that release each
/// iteration has its own copy of the range value, so its address differs.
pub fn applies(pass: &Pass<'_>) -> bool {
    !go_version_at_least(pass, 1, 22)
}

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn new(pass: &'a Pass<'a>) -> Self {
        Self {
            pass,
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
            let value_is_ptr = self
                .pass
                .types_info()
                .and_then(|info| info.types.get(&value.id()))
                .map(|t| type_string(self.pass, t.typ).starts_with('*'))
                .unwrap_or(false);
            inspect_range_body(&range.body, value, value_is_ptr, &mut self.failures);
            true
        });
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    if !applies(pass) {
        return Vec::new();
    }
    let mut c = Checker::new(pass);
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
