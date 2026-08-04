//! `empty-block` — warn on empty code blocks.

use guff::ast::{BlockStmt, Expr, RangeStmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

const MESSAGE: &str = "this block is empty, you can remove it";

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    let mut ignore = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::FuncDecl(f) => {
                    if let Some(body) = &f.body {
                        ignore.push(body as *const BlockStmt);
                    }
                }
                NodeRef::FuncLit(f) => {
                    ignore.push(&f.body as *const BlockStmt);
                }
                NodeRef::SelectStmt(s) => {
                    ignore.push(&s.body as *const BlockStmt);
                }
                NodeRef::ForStmt(f) => {
                    if f.init.is_none()
                        && f.post.is_none()
                        && f.cond.is_some()
                        && matches!(f.cond.as_ref(), Some(Expr::CallExpr(_)))
                        && f.body.list.is_empty()
                    {
                        ignore.push(&f.body as *const BlockStmt);
                    }
                }
                NodeRef::RangeStmt(r) => {
                    check_range(r, &mut failures);
                    return false;
                }
                NodeRef::BlockStmt(b) => {
                    check_block(b, &ignore, &mut failures);
                }
                _ => {}
            }
            true
        });
    }
    failures
}

fn check_range(r: &RangeStmt, failures: &mut Vec<Failure>) {
    // Upstream revive flags every empty range body (including `for range x {}`).
    if !r.body.list.is_empty() {
        return;
    }
    failures.push(Failure {
        rule: "empty-block",
        pos: r.for_.0 as u32,
        message: MESSAGE.into(),
        confidence: None,
    });
}

fn check_block(b: &BlockStmt, ignore: &[*const BlockStmt], failures: &mut Vec<Failure>) {
    let ptr = b as *const BlockStmt;
    if ignore.iter().any(|p| *p == ptr) {
        return;
    }
    if b.list.is_empty() {
        failures.push(Failure {
            rule: "empty-block",
            pos: b.lbrace.0 as u32,
            message: MESSAGE.into(),
            confidence: None,
        });
    }
}
