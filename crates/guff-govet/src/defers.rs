//! `defers` — check for missing `defer` on `time.Since` calls.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Decl, Expr, GenDecl, Spec};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::expreq::unparen;

fn imports_package(pass: &Pass<'_>, import_path: &str) -> bool {
    if pass.pkg().imports.contains_key(import_path) {
        return true;
    }
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::GenDecl(GenDecl { tok, specs, .. }) = decl else {
                continue;
            };
            if *tok != Some(Token::IMPORT) {
                continue;
            }
            for spec in specs {
                let Spec::ImportSpec(is) = spec else {
                    continue;
                };
                let trimmed = is.path.value.trim_matches('"');
                if trimmed == import_path {
                    return true;
                }
            }
        }
    }
    false
}

fn is_time_since(pass: &Pass<'_>, fun: &Expr) -> bool {
    match unparen(fun) {
        Expr::SelectorExpr(sel) => {
            guff_analysis::code::call_name(pass, &Expr::SelectorExpr(sel.clone()))
                .as_deref()
                == Some("time.Since")
        }
        _ => false,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if !imports_package(pass, "time") {
        return Ok(None);
    }

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "defers requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::DeferStmt(defer) = n else {
            return;
        };
        walk_defer_call(pass, &defer.call, &mut pending);
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn walk_defer_call(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    if is_time_since(pass, &call.fun) {
        pending.push((
            call.lparen.0 as u32,
            "call to time.Since is not deferred".into(),
        ));
    }
    walk_defer_expr(pass, &call.fun, pending);
    for arg in &call.args {
        walk_defer_expr(pass, arg, pending);
    }
}

fn walk_defer_expr(pass: &Pass<'_>, expr: &Expr, pending: &mut Vec<(u32, String)>) {
    match expr {
        Expr::CallExpr(call) => walk_defer_call(pass, call, pending),
        Expr::FuncLit(_) => {}
        Expr::UnaryExpr(u) => walk_defer_expr(pass, &u.x, pending),
        Expr::BinaryExpr(b) => {
            walk_defer_expr(pass, &b.x, pending);
            walk_defer_expr(pass, &b.y, pending);
        }
        Expr::SelectorExpr(s) => walk_defer_expr(pass, &s.x, pending),
        Expr::IndexExpr(i) => {
            walk_defer_expr(pass, &i.x, pending);
            walk_defer_expr(pass, &i.index, pending);
        }
        Expr::SliceExpr(s) => {
            walk_defer_expr(pass, &s.x, pending);
            for bound in [&s.low, &s.high, &s.max] {
                if let Some(e) = bound {
                    walk_defer_expr(pass, e, pending);
                }
            }
        }
        Expr::TypeAssertExpr(t) => walk_defer_expr(pass, &t.x, pending),
        Expr::StarExpr(s) => walk_defer_expr(pass, &s.x, pending),
        Expr::ParenExpr(p) => walk_defer_expr(pass, &p.x, pending),
        _ => {}
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "defers",
        doc: "report calls to time.Since inside defer that are not themselves deferred",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/defers",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
