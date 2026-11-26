//! `constant-logical-expr` — warn on constant logical expressions (e.g. `a == a`).

use guff::ast::BinaryExpr;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::expr_equal;

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
                    let NodeRef::BinaryExpr(expr) = n else { return; };
                    check_binary(expr, &mut self.failures);
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


fn is_operator_with_logical_result(op: Token) -> bool {
    matches!(
        op,
        Token::LAND
            | Token::LOR
            | Token::EQL
            | Token::LSS
            | Token::GTR
            | Token::NEQ
            | Token::LEQ
            | Token::GEQ
    )
}

fn is_equality_operator(op: Token) -> bool {
    matches!(op, Token::EQL | Token::LEQ | Token::GEQ)
}

fn is_inequality_operator(op: Token) -> bool {
    matches!(op, Token::LSS | Token::GTR | Token::NEQ)
}

fn check_binary(expr: &BinaryExpr, failures: &mut Vec<Failure>) {
    let op = expr.op;
    if !is_operator_with_logical_result(op) {
        return;
    }
    if !expr_equal(&expr.x, &expr.y) {
        return;
    }
    let message = if is_equality_operator(op) {
        "expression always evaluates to true"
    } else if is_inequality_operator(op) {
        "expression always evaluates to false"
    } else {
        "left and right hand-side sub-expressions are the same"
    };
    failures.push(Failure {
        rule: "constant-logical-expr",
        pos: expr.x.pos().0 as u32,
        message: message.into(),
        ..Failure::default()
    });
}
