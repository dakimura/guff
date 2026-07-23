//! `identical-switch-conditions` — warn on duplicate conditions in untagged switch statements.

use guff::ast::{CaseClause, Stmt, SwitchStmt};
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
                    let NodeRef::SwitchStmt(sw) = n else { return; };
                    check_switch(self.pass, sw, &mut self.failures);
        
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


fn check_switch(pass: &Pass<'_>, sw: &SwitchStmt, failures: &mut Vec<Failure>) {
    if sw.tag.is_some() {
        return;
    }
    let mut hashes = std::collections::HashMap::new();
    for stmt in &sw.body.list {
        let Stmt::CaseClause(case) = stmt else {
            continue;
        };
        let case_line = line_of(pass, case.case.0);
        for expr in &case.list {
            let hash = expr_fmt(expr);
            if let Some(prev) = hashes.get(&hash) {
                failures.push(Failure {
                    rule: "identical-switch-conditions",
                    pos: case.case.0 as u32,
                    message: format!("case clause at line {prev} has the same condition"),
            confidence: None,
        });
            } else {
                hashes.insert(hash, case_line);
            }
        }
    }
}
