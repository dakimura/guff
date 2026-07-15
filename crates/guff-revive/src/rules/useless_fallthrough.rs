//! `useless-fallthrough` — warn on redundant `fallthrough` in switch cases.

use guff::ast::{BranchStmt, CaseClause, Stmt, SwitchStmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::SwitchStmt(sw)) = n else {
                return true;
            };
            check_switch(sw, &mut failures);
            false
        });
    }
    failures
}

fn check_switch(sw: &SwitchStmt, failures: &mut Vec<Failure>) {
    if sw.tag.is_none() {
        return;
    }
    let cases: Vec<&CaseClause> = sw
        .body
        .list
        .iter()
        .filter_map(|stmt| {
            if let Stmt::CaseClause(c) = stmt {
                Some(c)
            } else {
                None
            }
        })
        .collect();
    for i in 0..cases.len().saturating_sub(1) {
        let case = cases[i];
        if case.body.len() != 1 {
            continue;
        }
        let Stmt::BranchStmt(BranchStmt { tok, .. }) = &case.body[0] else {
            continue;
        };
        if *tok != Token::FALLTHROUGH {
            continue;
        }
        let next = cases[i + 1];
        if next.list.is_empty() {
            continue;
        }
        failures.push(Failure {
            rule: "useless-fallthrough",
            pos: case.body[0].pos().0 as u32,
            message: r#"this "fallthrough" can be removed by consolidating this case clause with the next one"#.into(),
        });
    }
}
