//! `defer` — warn on common defer gotchas.

use guff::ast::{CallExpr, Expr, FuncLit, Stmt};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_ident, unparen};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::FuncDecl(f) = decl else {
                continue;
            };
            if let Some(body) = &f.body {
                visit_block(&body.list, false, false, 0, &mut failures);
            }
        }
    }
    failures
}

fn visit_block(
    stmts: &[Stmt],
    in_defer: bool,
    in_loop: bool,
    func_lit_depth: u8,
    failures: &mut Vec<Failure>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::ForStmt(f) => visit_block(&f.body.list, in_defer, true, func_lit_depth, failures),
            Stmt::RangeStmt(r) => visit_block(&r.body.list, in_defer, true, func_lit_depth, failures),
            Stmt::ReturnStmt(ret) => {
                if !ret.results.is_empty() && in_defer && func_lit_depth == 1 {
                    failures.push(Failure {
                        rule: "defer",
                        pos: ret.return_.0 as u32,
                        message: "return in a defer function has no effect".into(),
                    });
                }
            }
            Stmt::DeferStmt(d) => {
                check_defer(&d.call, in_loop, failures);
                visit_deferred_call(&d.call, in_loop, failures);
            }
            Stmt::ExprStmt(e) => {
                if let Expr::CallExpr(call) = &e.x {
                    check_recover_call(call, in_defer, func_lit_depth, failures);
                }
            }
            Stmt::BlockStmt(b) => visit_block(&b.list, in_defer, in_loop, func_lit_depth, failures),
            _ => {}
        }
    }
}

fn visit_deferred_call(call: &CallExpr, in_loop: bool, failures: &mut Vec<Failure>) {
    visit_deferred_expr(&call.fun, in_loop, failures);
    for arg in &call.args {
        if matches!(arg, Expr::FuncLit(_)) {
            continue;
        }
        visit_deferred_expr(arg, in_loop, failures);
    }
}

fn visit_deferred_expr(expr: &Expr, in_loop: bool, failures: &mut Vec<Failure>) {
    match unparen(expr) {
        Expr::FuncLit(FuncLit { body, .. }) => {
            visit_block(&body.list, true, in_loop, 1, failures);
        }
        Expr::CallExpr(call) => {
            check_recover_call(call, true, 0, failures);
            for arg in &call.args {
                visit_deferred_expr(arg, in_loop, failures);
            }
        }
        _ => {}
    }
}

fn check_defer(call: &CallExpr, in_loop: bool, failures: &mut Vec<Failure>) {
    if is_ident(&call.fun, "recover") {
        failures.push(Failure {
            rule: "defer",
            pos: call.fun.pos().0 as u32,
            message: "recover must be called inside a deferred function, this is executing recover immediately".into(),
        });
    }
    if in_loop {
        failures.push(Failure {
            rule: "defer",
            pos: call.fun.pos().0 as u32,
            message: "prefer not to defer inside loops".into(),
        });
    }
    if matches!(unparen(&call.fun), Expr::CallExpr(_)) {
        failures.push(Failure {
            rule: "defer",
            pos: call.fun.pos().0 as u32,
            message: "prefer not to defer chains of function calls".into(),
        });
    }
}

fn check_recover_call(call: &CallExpr, in_defer: bool, func_lit_depth: u8, failures: &mut Vec<Failure>) {
    if !is_ident(&call.fun, "recover") {
        return;
    }
    if !in_defer {
        failures.push(Failure {
            rule: "defer",
            pos: call.fun.pos().0 as u32,
            message: "recover must be called inside a deferred function".into(),
        });
    } else if func_lit_depth == 0 {
        failures.push(Failure {
            rule: "defer",
            pos: call.fun.pos().0 as u32,
            message: "recover must be called inside a deferred function, this is executing recover immediately".into(),
        });
    }
}
