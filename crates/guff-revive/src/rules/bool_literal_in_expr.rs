//! `bool-literal-in-expr` — warn on boolean literals in logic expressions.

use guff::ast::{BinaryExpr, Expr, Ident};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::BinaryExpr(expr)) = n else {
                return true;
            };
            check_binary(expr, &mut failures);
            true
        });
    }
    failures
}

fn is_bool_op(op: Token) -> bool {
    matches!(op, Token::LAND | Token::LOR | Token::EQL | Token::NEQ)
}

fn bool_lit(expr: &Expr) -> Option<&'static str> {
    let Expr::Ident(Ident { name, .. }) = expr else {
        return None;
    };
    match name.as_str() {
        "true" => Some("true"),
        "false" => Some("false"),
        _ => None,
    }
}

fn check_binary(expr: &BinaryExpr, failures: &mut Vec<Failure>) {
    let op = expr.op;
    if !is_bool_op(op) {
        return;
    }
    let lexeme = bool_lit(&expr.x).or_else(|| bool_lit(&expr.y));
    let Some(lexeme) = lexeme else {
        return;
    };
    let message = if (op == Token::LAND && lexeme == "false") || (op == Token::LOR && lexeme == "true")
    {
        format!("Boolean expression seems to always evaluate to {lexeme}")
    } else {
        "omit Boolean literal in expression".into()
    };
    failures.push(Failure {
        rule: "bool-literal-in-expr",
        pos: expr.x.pos().0 as u32,
        message,
            confidence: None,
        });
}
