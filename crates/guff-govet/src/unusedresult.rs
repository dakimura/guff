//! `unusedresult` — check for unused results of important stdlib calls.

use std::sync::OnceLock;

use guff::ast::{Expr, ExprStmt, SelectorExpr};
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::expreq::unparen;

fn is_must_use_call(fun: &Expr) -> Option<&'static str> {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(fun) else {
        return None;
    };
    let Expr::Ident(pkg) = x.as_ref() else {
        return None;
    };
    match (pkg.name.as_str(), sel.name.as_str()) {
        ("errors", "New") => Some("errors.New"),
        ("fmt", "Errorf") => Some("fmt.Errorf"),
        ("fmt", "Sprint") => Some("fmt.Sprint"),
        ("fmt", "Sprintf") => Some("fmt.Sprintf"),
        ("fmt", "Sprintln") => Some("fmt.Sprintln"),
        ("context", "WithCancel") => Some("context.WithCancel"),
        ("context", "WithDeadline") => Some("context.WithDeadline"),
        ("context", "WithTimeout") => Some("context.WithTimeout"),
        ("context", "WithValue") => Some("context.WithValue"),
        ("sort", "Reverse") => Some("sort.Reverse"),
        _ => None,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "unusedresult requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::ExprStmt(ExprStmt { x, .. }) = n else {
            return;
        };
        let Expr::CallExpr(call) = &x else {
            return;
        };
        if let Some(name) = is_must_use_call(&call.fun) {
            pending.push((call.lparen.0 as u32, format!("result of {name} call not used")));
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
        name: "unusedresult",
        doc: "check for unused results of calls to certain functions",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/unusedresult",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
