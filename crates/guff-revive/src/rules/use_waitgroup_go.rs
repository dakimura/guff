//! `use-waitgroup-go` (revive v1.17.0) — `wg.Add(…)` followed by a `go func()
//! { … wg.Done() … }()` (directly, or as a direct child of a `for`/`range`
//! body) could be `wg.Go(…)`. Name-based on purpose upstream: only a variable
//! called `wg` is recognised, so no type information is needed. Go 1.25+.

use guff::ast::{BlockStmt, Expr, GoStmt, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{go_version_at_least, is_pkg_dot_name};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    if !go_version_at_least(pass, 1, 25) {
        return Vec::new();
    }
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::FuncDecl(fd) = decl else { continue };
            let Some(body) = &fd.body else { continue };
            walk::inspect(NodeRef::BlockStmt(body), |n| {
                if let Some(NodeRef::BlockStmt(b)) = n {
                    analyze_block(b, &mut failures);
                }
                true
            });
        }
    }
    failures
}

/// `analyzeBlock`: each `wg.Add` that some later statement of the block
/// answers with a goroutine calling `wg.Done`; the search for the next `Add`
/// resumes after that statement.
fn analyze_block(b: &BlockStmt, failures: &mut Vec<Failure>) {
    let stmts = &b.list;
    let mut i = 0;
    while i < stmts.len() {
        if !is_call_to_wg_add(&stmts[i]) {
            i += 1;
            continue;
        }
        let call = &stmts[i];
        i += 1;
        while i < stmts.len() {
            if find_go_stmt_with_wg_done(&stmts[i]).is_some() {
                failures.push(Failure {
                    rule: "use-waitgroup-go",
                    pos: call.pos().0 as u32,
                    message: "replace wg.Add()...go {...wg.Done()...} with wg.Go(...)".into(),
                    ..Failure::default()
                });
                break;
            }
            i += 1;
        }
        i += 1;
    }
}

fn find_go_stmt_with_wg_done(stmt: &Stmt) -> Option<&GoStmt> {
    match stmt {
        Stmt::GoStmt(g) if has_call_to_wg_done(g) => Some(g),
        Stmt::ForStmt(f) => seek_in_block(&f.body),
        Stmt::RangeStmt(r) => seek_in_block(&r.body),
        _ => None,
    }
}

fn seek_in_block(b: &BlockStmt) -> Option<&GoStmt> {
    b.list.iter().find_map(|s| match s {
        Stmt::GoStmt(g) if has_call_to_wg_done(g) => Some(g),
        _ => None,
    })
}

/// The goroutine runs a function literal whose body calls `wg.Done`.
fn has_call_to_wg_done(g: &GoStmt) -> bool {
    let Expr::FuncLit(lit) = g.call.fun.as_ref() else {
        return false;
    };
    let mut found = false;
    walk::inspect(NodeRef::BlockStmt(&lit.body), |n| {
        if let Some(NodeRef::CallExpr(c)) = n {
            found |= is_pkg_dot_name(&c.fun, "wg", "Done");
        }
        !found
    });
    found
}

fn is_call_to_wg_add(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::ExprStmt(e) if matches!(&e.x, Expr::CallExpr(c) if is_pkg_dot_name(&c.fun, "wg", "Add")))
}
