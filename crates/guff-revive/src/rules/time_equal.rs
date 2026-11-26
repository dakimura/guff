//! `time-equal` — warn on `==` / `!=` comparisons of `time.Time` values.

use guff::ast::{BinaryExpr, Expr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_named_type, render_node, type_of};

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
                    let NodeRef::BinaryExpr(expr) = n else { return; };
                    check_binary(self.pass, expr, &mut self.failures);
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
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
            // `%q` on a `token.Token` quotes its `String()`, so this is
            // `"=="` / `"!="` — not the token's Go identifier.
            "use {negate}{}.Equal({}) instead of \"{}\" operator",
            render_node(pass, &expr.x),
            render_node(pass, &expr.y),
            op
        ),
        ..Failure::default()
    });
}
