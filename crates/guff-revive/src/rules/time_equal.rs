//! `time-equal` — warn on `==` / `!=` comparisons of `time.Time` values.

use guff::ast::{BinaryExpr, Expr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{expr_string, is_named_type, type_of};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::BinaryExpr(expr)) = n else {
                return true;
            };
            check_binary(pass, expr, &mut failures);
            true
        });
    }
    failures
}

fn check_binary(pass: &Pass<'_>, expr: &BinaryExpr, failures: &mut Vec<Failure>) {
    let op = expr.op;
    if !matches!(op, Token::EQL | Token::NEQ) {
        return;
    }
    let (Some(x_ty), Some(y_ty)) = (type_of(pass, &expr.x), type_of(pass, &expr.y)) else {
        return;
    };
    if !is_named_type(pass, x_ty, "time", "Time") || !is_named_type(pass, y_ty, "time", "Time") {
        return;
    }
    let negate = if op == Token::NEQ { "!" } else { "" };
    failures.push(Failure {
        rule: "time-equal",
        pos: expr.x.pos().0 as u32,
        message: format!(
            "use {negate}{}.Equal({}) instead of {:?} operator",
            expr_string(&expr.x),
            expr_string(&expr.y),
            op
        ),
            confidence: None,
        });
}
