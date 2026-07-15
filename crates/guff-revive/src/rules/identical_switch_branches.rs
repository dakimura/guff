//! `identical-switch-branches` — warn on tagged switch statements with identical case bodies.

use guff::ast::{BranchStmt, CaseClause, Stmt, SwitchStmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::astfmt::stmts_fmt;
use crate::failure::Failure;
use crate::util::line_of;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::SwitchStmt(sw)) = n else {
                return true;
            };
            check_switch(pass, sw, &mut failures);
            false
        });
    }
    failures
}

fn check_switch(pass: &Pass<'_>, sw: &SwitchStmt, failures: &mut Vec<Failure>) {
    if sw.tag.is_none() {
        return;
    }
    let mut hashes = std::collections::HashMap::new();
    for stmt in &sw.body.list {
        let Stmt::CaseClause(case) = stmt else {
            continue;
        };
        if ends_with_fallthrough(case) {
            continue;
        }
        let hash = stmts_fmt(&case.body);
        let line = line_of(pass, case.case.0);
        if let Some(prev) = hashes.get(&hash) {
            failures.push(Failure {
                rule: "identical-switch-branches",
                pos: sw.switch.0 as u32,
                message: format!(
                    "\"switch\" with identical branches (lines {prev} and {line})"
                ),
            });
        } else {
            hashes.insert(hash, line);
        }
    }
}

fn ends_with_fallthrough(case: &CaseClause) -> bool {
    let Some(Stmt::BranchStmt(BranchStmt { tok, .. })) = case.body.last() else {
        return false;
    };
    *tok == Token::FALLTHROUGH
}
