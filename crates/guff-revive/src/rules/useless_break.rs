//! `useless-break` — warn on redundant `break` at the end of switch/select cases.

use guff::ast::{BranchStmt, CaseClause, CommClause, ForStmt, RangeStmt, Stmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| match n {
            Some(NodeRef::ForStmt(for_stmt)) => {
                inspect_loop_body(&for_stmt.body.list, false, &mut failures);
                false
            }
            Some(NodeRef::RangeStmt(range_stmt)) => {
                inspect_loop_body(&range_stmt.body.list, false, &mut failures);
                false
            }
            Some(NodeRef::CaseClause(case)) => {
                inspect_case_body(&case.body, false, &mut failures);
                false
            }
            Some(NodeRef::CommClause(case)) => {
                inspect_case_body(&case.body, false, &mut failures);
                false
            }
            _ => true,
        });
    }
    failures
}

fn inspect_loop_body(body: &[Stmt], in_loop: bool, failures: &mut Vec<Failure>) {
    for stmt in body {
        match stmt {
            Stmt::ForStmt(f) => {
                inspect_loop_body(&f.body.list, true, failures);
            }
            Stmt::RangeStmt(r) => {
                inspect_loop_body(&r.body.list, true, failures);
            }
            Stmt::SwitchStmt(s) => {
                for case in &s.body.list {
                    if let Stmt::CaseClause(c) = case {
                        inspect_case_body(&c.body, in_loop, failures);
                    }
                }
            }
            Stmt::SelectStmt(s) => {
                for comm in &s.body.list {
                    if let Stmt::CommClause(c) = comm {
                        inspect_case_body(&c.body, in_loop, failures);
                    }
                }
            }
            _ => {}
        }
    }
}

fn inspect_case_body(body: &[Stmt], in_loop: bool, failures: &mut Vec<Failure>) {
    let Some(last) = body.last() else {
        return;
    };
    let Stmt::BranchStmt(BranchStmt { tok, label, .. }) = last else {
        return;
    };
    if *tok != Token::BREAK || label.is_some() {
        return;
    }
    let mut msg = "useless break in case clause".to_string();
    if in_loop {
        msg.push_str(" (WARN: this break statement affects this switch or select statement and not the loop enclosing it)");
    }
    failures.push(Failure {
        rule: "useless-break",
        pos: last.pos().0 as u32,
        message: msg,
            confidence: None,
        });
}
