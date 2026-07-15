//! `redundant-test-main-exit` — warn on `os.Exit` in `TestMain`.

use guff::ast::{CallExpr, Decl, Expr, Ident, SelectorExpr, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_pkg_dot_name, is_test_package, unparen};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let file = match pass.files().first() {
        Some(f) => f,
        None => return Vec::new(),
    };
    if !is_test_package(&file.name.name) {
        return Vec::new();
    }

    let mut failures = Vec::new();
    for decl in &file.decls {
        let Decl::FuncDecl(f) = decl else {
            continue;
        };
        if f.name.name != "TestMain" {
            continue;
        }
        let Some(body) = &f.body else {
            continue;
        };
        walk::inspect(NodeRef::BlockStmt(body), |n| {
            let Some(NodeRef::ExprStmt(expr)) = n else {
                return true;
            };
            let Expr::CallExpr(call) = &expr.x else {
                return true;
            };
            if is_exit_call(call) {
                failures.push(Failure {
                    rule: "redundant-test-main-exit",
                    pos: call.fun.pos().0 as u32,
                    message: format!(
                        "redundant call to {} in TestMain function, the test runner will handle it automatically as of Go 1.15",
                        call_name(call)
                    ),
                });
            }
            true
        });
    }
    failures
}

fn is_exit_call(call: &CallExpr) -> bool {
    if is_pkg_dot_name(&call.fun, "flag", "Parse") {
        return false;
    }
    is_pkg_dot_name(&call.fun, "os", "Exit") || is_pkg_dot_name(&call.fun, "syscall", "Exit")
}

fn call_name(call: &CallExpr) -> String {
    match unparen(&call.fun) {
        Expr::SelectorExpr(SelectorExpr { x, sel, .. }) => {
            let pkg = match unparen(x) {
                Expr::Ident(Ident { name, .. }) => name.clone(),
                _ => "?".into(),
            };
            format!("{pkg}.{}", sel.name)
        }
        other => format!("{other:?}"),
    }
}

#[allow(unused_imports)]
use guff::ast::Stmt as _;
