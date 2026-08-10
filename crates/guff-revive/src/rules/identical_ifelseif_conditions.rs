//! `identical-ifelseif-conditions` — warn on duplicate conditions in if/else-if chains.

use guff::ast::{IfStmt, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::astfmt::expr_fmt;
use crate::failure::Failure;
use crate::util::line_of;

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
                    let NodeRef::IfStmt(if_stmt) = n else { return; };
                    if matches!(if_stmt.else_.as_deref(), Some(Stmt::IfStmt(_))) {
                        check_chain(self.pass, if_stmt, &mut self.failures);
                    }
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


fn check_chain(pass: &Pass<'_>, start: &IfStmt, failures: &mut Vec<Failure>) {
    let mut conditions = std::collections::HashMap::new();
    let mut current = Some(start);

    while let Some(if_stmt) = current {
        if if_stmt.init.is_none() {
            let hash = expr_fmt(&if_stmt.cond);
            let line = line_of(pass, if_stmt.if_.0);
            if let Some(prev) = conditions.get(&hash) {
                failures.push(Failure {
                    rule: "identical-ifelseif-conditions",
                    pos: if_stmt.if_.0 as u32,
                    message: format!(
                        "\"if...else if\" chain with identical conditions (lines {prev} and {line})"
                    ),
                    ..Failure::default()
                });
            } else {
                conditions.insert(hash, line);
            }
        }
        current = match if_stmt.else_.as_deref() {
            Some(Stmt::IfStmt(next)) => Some(next),
            _ => None,
        };
    }
}
