//! SA2000 — `(*sync.WaitGroup).Add` called inside the goroutine.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa2000`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Decl, Expr, FuncDecl, FuncLit, SelectorExpr, Stmt};
use guff_analysis::code::{is_call_to, is_of_type_with_name};
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::render::render_expr;

fn is_waitgroup_add(pass: &Pass<'_>, call: &CallExpr) -> bool {
    if is_call_to(pass, call, "(*sync.WaitGroup).Add") {
        return true;
    }
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen_expr(call.fun.as_ref()) else {
        return false;
    };
    if sel.name != "Add" {
        return false;
    }
    is_of_type_with_name(pass, x, "sync.WaitGroup")
        || is_of_type_with_name(pass, x, "*sync.WaitGroup")
}

fn find_waitgroup_add_in_body<'a>(
    pass: &Pass<'_>,
    body: &'a [Stmt],
) -> Option<&'a CallExpr> {
    for stmt in body {
        if let Stmt::ExprStmt(es) = stmt {
            if let Expr::CallExpr(call) = unparen_expr(&es.x) {
                if is_waitgroup_add(pass, call) {
                    return Some(call);
                }
            }
        }
        if let Stmt::BlockStmt(block) = stmt {
            if let Some(call) = find_waitgroup_add_in_body(pass, &block.list) {
                return Some(call);
            }
        }
    }
    None
}

fn check_go_call(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Expr::FuncLit(FuncLit { body, .. }) = unparen_expr(call.fun.as_ref()) else {
        return;
    };
    let Some(add_call) = find_waitgroup_add_in_body(pass, &body.list) else {
        return;
    };
    // Upstream renders the whole `Add` call and reports the call node, not the
    // callee alone at the `(`: `wgs[0].Add(2 + 1)`, verified against
    // golangci-lint 2.12.2.
    let rendered = render_expr(&Expr::CallExpr(add_call.clone()));
    pending.push((
        add_call.pos().0 as u32,
        format!("should call {rendered} before starting the goroutine to avoid a race"),
    ));
}

fn walk_stmts(pass: &Pass<'_>, stmts: &[Stmt], pending: &mut Vec<(u32, String)>) {
    for stmt in stmts {
        match stmt {
            Stmt::GoStmt(g) => check_go_call(pass, &g.call, pending),
            Stmt::BlockStmt(b) => walk_stmts(pass, &b.list, pending),
            Stmt::IfStmt(i) => {
                if let Some(init) = &i.init {
                    walk_stmts(pass, std::slice::from_ref(init), pending);
                }
                walk_stmts(pass, &i.body.list, pending);
                if let Some(else_) = &i.else_ {
                    walk_stmts(pass, std::slice::from_ref(else_), pending);
                }
            }
            Stmt::ForStmt(f) => {
                if let Some(init) = &f.init {
                    walk_stmts(pass, std::slice::from_ref(init), pending);
                }
                walk_stmts(pass, &f.body.list, pending);
            }
            Stmt::RangeStmt(r) => walk_stmts(pass, &r.body.list, pending),
            _ => {}
        }
    }
}

fn walk_func(pass: &Pass<'_>, decl: &FuncDecl, pending: &mut Vec<(u32, String)>) {
    if let Some(body) = &decl.body {
        walk_stmts(pass, &body.list, pending);
    }
}

fn unparen_expr(expr: &Expr) -> &Expr {
    match expr {
        Expr::ParenExpr(p) => unparen_expr(&p.x),
        other => other,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            if let Decl::FuncDecl(f) = decl {
                walk_func(pass, f, &mut pending);
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn sa2000_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA2000",
        doc: "WaitGroup.Add called inside the goroutine",
        url: "https://staticcheck.dev/docs/checks/#SA2000",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    }
}

/// SA2000 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa2000_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa2000_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
