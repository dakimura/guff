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
                    // Upstream prunes the subtree **only when the body is
                    // empty**, where the walk would otherwise reach the same
                    // `BlockStmt` and report it a second time:
                    //
                    //     case *ast.RangeStmt:
                    //         if len(n.Body.List) == 0 {
                    //             w.onFailure(…)
                    //             return nil // skip visiting the range subtree
                    //         }
                    //
                    // guff pruned at *every* range statement, so nothing inside
                    // a non-empty `for … range` was ever visited. k6's
                    // `ramping_arrival_rate_test.go:294` is an empty drain loop
                    // two closures deep inside `for _, tc := range tests`.
                    if r.body.list.is_empty() {
                        if r.key.is_none() && r.value.is_none() {
                            // revive v1.17.0: a bare `for range x {}` (a
                            // channel drain) is skipped, and the walk goes on.
                            ignore.push(&r.body as *const BlockStmt);
                            return true;
                        }
                        check_range(r, &mut failures);
                        return false;
                    }
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
    // An empty range body with a key or a value; the bare form is skipped by
    // the caller (revive v1.17.0).
    debug_assert!(r.body.list.is_empty());
    // Both empty-block sites report the same text, so the confidence cannot be
    // recovered from the message: the range arm is 0.9 upstream, the plain
    // block arm below is 1.
    failures.push(Failure::with_confidence(
        "empty-block",
        r.for_.0 as u32,
        MESSAGE,
        0.9,
    ));
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
            ..Failure::default()
        });
    }
}
