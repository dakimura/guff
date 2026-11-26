//! `atomic` — check for misuse of `sync/atomic` add functions.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Decl, Expr, GenDecl, Spec, StarExpr, UnaryExpr};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::expreq::{expr_equal, unparen};

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

fn is_atomic_add(pass: &Pass<'_>, fun: &Expr) -> bool {
    let Some(name) = guff_analysis::code::call_name(pass, fun) else {
        return false;
    };
    matches!(
        name.as_str(),
        "sync/atomic.AddInt32"
            | "sync/atomic.AddInt64"
            | "sync/atomic.AddUint32"
            | "sync/atomic.AddUint64"
            | "sync/atomic.AddUintptr"
    )
}

fn check_atomic_add_assignment(_pass: &Pass<'_>, left: &Expr, call: &CallExpr) -> bool {
    if call.args.len() != 2 {
        return false;
    }
    let arg = &call.args[0];
    match unparen(arg) {
        Expr::UnaryExpr(UnaryExpr { op: Token::AND, x, .. }) => expr_equal(left, x),
        _ => match unparen(left) {
            Expr::StarExpr(StarExpr { x, .. }) => expr_equal(x, arg),
            _ => false,
        },
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if !imports_package(pass, "sync/atomic") {
        return Ok(None);
    }

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "atomic requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(AssignStmt), pass.files(), |n| {
        let NodeRef::AssignStmt(AssignStmt { tok, lhs, rhs, .. }) = n else {
            return;
        };
        if lhs.len() != rhs.len() {
            return;
        }
        if lhs.len() == 1 && *tok == Some(Token::DEFINE) {
            return;
        }
        for (left, right) in lhs.iter().zip(rhs) {
            let Expr::CallExpr(call) = right else {
                continue;
            };
            if !is_atomic_add(pass, &call.fun) {
                continue;
            }
            if check_atomic_add_assignment(pass, left, call) {
                pending.push((left.pos().0 as u32, "direct assignment to atomic value".into()));
            }
        }
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "atomic",
        doc: "check for common mistakes using the sync/atomic package",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/atomic",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
