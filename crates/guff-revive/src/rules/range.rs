//! `range` — omit redundant `_` value in range loops.

use guff::ast::{Expr, RangeStmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_blank_ident;

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
                    let NodeRef::RangeStmt(rs) = n else { return; };
                    check_range(rs, &mut self.failures);
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
