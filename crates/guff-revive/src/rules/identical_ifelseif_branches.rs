//! `identical-ifelseif-branches` — warn on if/else-if chains with identical branches.

use guff::ast::{Expr, IfStmt, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::astfmt::{block_fmt, stmt_fmt};
use crate::failure::Failure;
use crate::util::line_of;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::IfStmt(if_stmt)) = n else {
                return true;
            };
            if matches!(if_stmt.else_.as_deref(), Some(Stmt::IfStmt(_))) {
                check_chain(pass, if_stmt, &mut failures);
            }
            true
        });
    }
    failures
}

fn check_chain(pass: &Pass<'_>, start: &IfStmt, failures: &mut Vec<Failure>) {
    let mut branches: Vec<(String, usize)> = Vec::new();
    let mut has_complex_condition = false;
    let mut current = Some(start);

    while let Some(if_stmt) = current {
        if if_stmt.init.is_none() {
            branches.push((
                block_fmt(&if_stmt.body),
                line_of(pass, if_stmt.body.lbrace.0),
            ));
        }
        if condition_has_call(&if_stmt.cond) {
            has_complex_condition = true;
        }
        match if_stmt.else_.as_deref() {
            Some(Stmt::IfStmt(next)) => current = Some(next),
            Some(other) => {
                branches.push((stmt_fmt(other), line_of(pass, other.pos().0)));
                current = None;
            }
            None => current = None,
        }
    }

    for (a, b) in identical_branch_pairs(&branches) {
        let _ = has_complex_condition;
        failures.push(Failure {
            rule: "identical-ifelseif-branches",
            pos: start.if_.0 as u32,
            message: format!(
                "\"if...else if\" chain with identical branches (lines {a} and {b})"
            ),
            confidence: None,
        });
    }
}

fn identical_branch_pairs(branches: &[(String, usize)]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if branches.len() < 2 {
        return out;
    }
    let mut hashes = std::collections::HashMap::new();
    for (hash, line) in branches {
        if let Some(prev) = hashes.get(hash) {
            out.push((*prev, *line));
        } else {
            hashes.insert(hash.clone(), *line);
        }
    }
    out
}

fn condition_has_call(cond: &guff::ast::Expr) -> bool {
    match cond {
        Expr::CallExpr(_) => true,
        Expr::BinaryExpr(b) => condition_has_call(&b.x) || condition_has_call(&b.y),
        Expr::UnaryExpr(u) => condition_has_call(&u.x),
        Expr::ParenExpr(p) => condition_has_call(&p.x),
        _ => false,
    }
}
