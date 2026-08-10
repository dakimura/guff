//! `optimize-operands-order` — suggest swapping boolean operands for cheaper evaluation.

use guff::ast::{BinaryExpr, CallExpr, Expr, Ident};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::astfmt::expr_fmt;
use crate::failure::Failure;
use crate::util::unparen;

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
                    let NodeRef::BinaryExpr(bin) = n else { return; };
                    if !matches!(bin.op, Token::LAND | Token::LOR) {
                        return;
                    }
                    if !subexpr_has_call(&bin.x) {
                        return;
                    }
                    if subexpr_has_call(&bin.y) {
                        return;
                    }
                    let swapped = Expr::BinaryExpr(BinaryExpr {
                        x: bin.y.clone(),
                        op_pos: bin.op_pos,
                        op: bin.op,
                        y: bin.x.clone(),
                        id: bin.id,
                    });
                    self.failures.push(Failure {
                        rule: "optimize-operands-order",
                        pos: bin.x.pos().0 as u32,
                        message: format!(
                            "for better performance '{}' might be rewritten as '{}'",
                            expr_fmt(&Expr::BinaryExpr(bin.clone())),
                            expr_fmt(&swapped)
                        ),
                        ..Failure::default()
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


fn subexpr_has_call(expr: &Expr) -> bool {
    match expr {
        Expr::CallExpr(call) => !is_cheap_len_call(call),
        Expr::BinaryExpr(b) => subexpr_has_call(&b.x) || subexpr_has_call(&b.y),
        Expr::UnaryExpr(u) => subexpr_has_call(&u.x),
        Expr::ParenExpr(p) => subexpr_has_call(&p.x),
        _ => false,
    }
}

fn is_cheap_len_call(call: &CallExpr) -> bool {
    matches!(unparen(&call.fun), Expr::Ident(Ident { name, .. }) if name == "len")
}
