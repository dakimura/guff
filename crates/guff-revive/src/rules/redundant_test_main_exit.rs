//! `redundant-test-main-exit` — warn on `os.Exit` in `TestMain`.

use guff::ast::{CallExpr, Expr, Ident, SelectorExpr};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_pkg_dot_name, is_test_package, unparen};

pub struct Checker {
    failures: Vec<Failure>,
}

impl Checker {
    pub fn try_new(pass: &Pass<'_>) -> Option<Self> {
        let file = pass.files().first()?;
        if !is_test_package(&file.name.name) {
            return None;
        }
        Some(Self {
            failures: Vec::new(),
        })
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::FuncDecl(f) = n else {
            return;
        };
        if f.name.name != "TestMain" {
            return;
        }
        let Some(body) = &f.body else {
            return;
        };
        walk::inspect(NodeRef::BlockStmt(body), |inner| {
            let Some(NodeRef::ExprStmt(expr)) = inner else {
                return true;
            };
            let Expr::CallExpr(call) = &expr.x else {
                return true;
            };
            if is_exit_call(call) {
                self.failures.push(Failure {
                    rule: "redundant-test-main-exit",
                    pos: call.fun.pos().0 as u32,
                    message: format!(
                        "redundant call to {} in TestMain function, the test runner will handle it automatically as of Go 1.15",
                        call_name(call)
                    ),
                    confidence: None,
                });
            }
            true
        });
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let Some(mut c) = Checker::try_new(pass) else {
        return Vec::new();
    };
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
