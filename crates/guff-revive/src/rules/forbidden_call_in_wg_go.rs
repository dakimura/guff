//! `forbidden-call-in-wg-go` — forbid `panic` / `wg.Done` inside `wg.Go` callbacks.

use guff::ast::{CallExpr, Expr, FuncLit, Ident, SelectorExpr};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{go_version_at_least, is_ident, unparen};

pub struct Checker {
    failures: Vec<Failure>,
}

impl Checker {
    pub fn try_new(pass: &Pass<'_>) -> Option<Self> {
        if !go_version_at_least(pass, 1, 25) {
            return None;
        }
        Some(Self {
            failures: Vec::new(),
        })
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if !is_ident_dot_name(&call.fun, "wg", "Go") || call.args.len() != 1 {
            return;
        }
        let Expr::FuncLit(FuncLit { body, .. }) = unparen(&call.args[0]) else {
            return;
        };
        walk::inspect(NodeRef::BlockStmt(body), |inner| {
            let Some(NodeRef::CallExpr(inner_call)) = inner else {
                return true;
            };
            if let Some(callee) = forbidden_callee(inner_call) {
                self.failures.push(Failure {
                    rule: "forbidden-call-in-wg-go",
                    pos: inner_call.pos().0 as u32,
                    message: format!("do not call {callee} inside wg.Go"),
                    ..Failure::default()
                });
                return false;
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

fn is_ident_dot_name(fun: &Expr, recv: &str, name: &str) -> bool {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(fun) else {
        return false;
    };
    matches!(unparen(x), Expr::Ident(Ident { name: n, .. }) if n == recv) && sel.name == name
}

fn forbidden_callee(call: &CallExpr) -> Option<String> {
    if is_ident(&call.fun, "panic") {
        return Some("panic".into());
    }
    if is_ident_dot_name(&call.fun, "wg", "Done") {
        return Some("wg.Done".into());
    }
    if is_pkg_dot_name(&call.fun, "log", "Panic")
        || is_pkg_dot_name(&call.fun, "log", "Panicf")
        || is_pkg_dot_name(&call.fun, "log", "Panicln")
    {
        return Some(call_name(&call.fun));
    }
    None
}

fn is_pkg_dot_name(fun: &Expr, pkg: &str, name: &str) -> bool {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(fun) else {
        return false;
    };
    matches!(unparen(x), Expr::Ident(Ident { name: p, .. }) if p == pkg) && sel.name == name
}

fn call_name(fun: &Expr) -> String {
    match unparen(fun) {
        Expr::Ident(Ident { name, .. }) => name.clone(),
        Expr::SelectorExpr(SelectorExpr { x, sel, .. }) => {
            format!("{}.{}", call_name(x), sel.name)
        }
        _ => "<call>".into(),
    }
}
