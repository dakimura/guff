//! `range` — omit redundant `_` value in range loops.

use guff::ast::{Expr, RangeStmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_blank_ident;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::RangeStmt(rs)) = n else {
                return true;
            };
            check_range(rs, &mut failures);
            true
        });
    }
    failures
}

fn check_range(rs: &RangeStmt, failures: &mut Vec<Failure>) {
    if rs.value.is_none() {
        return;
    }
    let Some(value) = &rs.value else {
        return;
    };
    if !is_blank_ident(value) {
        return;
    }
    let key = rs
        .key
        .as_ref()
        .map(|k| match k {
            Expr::Ident(id) => id.name.clone(),
            _ => "_".into(),
        })
        .unwrap_or_else(|| "_".into());
    let tok = match rs.tok {
        Some(Token::DEFINE) => ":=",
        Some(Token::ASSIGN) | None => "=",
        _ => "=",
    };
    failures.push(Failure {
        rule: "range",
        pos: value.pos().0 as u32,
        message: format!(
            "should omit 2nd value from range; this loop is equivalent to `for {key} {tok} range ...`"
        ),
            confidence: None,
        });
}
