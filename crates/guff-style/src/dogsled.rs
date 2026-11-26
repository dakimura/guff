//! Port of [`github.com/alexkohler/dogsled`](https://github.com/alexkohler/dogsled)
//! (golangci-lint rewrite in `pkg/golinters/dogsled`).
//!
//! Default matches golangci-lint: `max-blank-identifiers=2`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, FuncDecl, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::DogsledOptions;

fn blank_count(assign: &AssignStmt) -> usize {
    assign
        .lhs
        .iter()
        .filter(|e| matches!(e, Expr::Ident(id) if id.name == "_"))
        .count()
}

fn check_func(func: &FuncDecl, max_blank: usize, pending: &mut Vec<(u32, String)>) {
    let Some(body) = &func.body else {
        return;
    };
    for stmt in &body.list {
        let Stmt::AssignStmt(assign) = stmt else {
            continue;
        };
        let n = blank_count(assign);
        if n > max_blank {
            let pos = assign
                .lhs
                .first()
                .map(|e| e.pos().0 as u32)
                .unwrap_or(assign.tok_pos.0 as u32);
            pending.push((pos, format!("declaration has {n} blank identifiers")));
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "dogsled requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<DogsledOptions>("dogsled")
        .copied()
        .unwrap_or_default();
    let max_blank = options.max_blank_identifiers;

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::FuncDecl(f) = n {
                check_func(f, max_blank, &mut pending);
            }
            true
        });
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "dogsled",
        doc: "checks assignments with too many blank identifiers (e.g. x, _, _, _ := f())",
        url: "https://github.com/alexkohler/dogsled",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
