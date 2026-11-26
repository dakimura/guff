//! Port of [`github.com/leighmcculloch/gochecknoinits`](https://github.com/leighmcculloch/gochecknoinits).
//!
//! Reports the use of `init()` functions.

use std::sync::OnceLock;

use guff::ast::FuncDecl;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

/// Number of fields declared in an optional receiver list (Go's `Recv.NumFields()`).
fn recv_num_fields(func: &FuncDecl) -> usize {
    match &func.recv {
        Some(list) => list.list.iter().map(|f| f.names.len().max(1)).sum(),
        None => 0,
    }
}

fn check_func(func: &FuncDecl, pending: &mut Vec<(u32, String)>) {
    if func.name.name == "init" && recv_num_fields(func) == 0 {
        pending.push((
            func.ty.pos().0 as u32,
            "don't use `init` function".to_string(),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "gochecknoinits requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            if let guff::ast::Decl::FuncDecl(f) = decl {
                check_func(f, &mut pending);
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "gochecknoinits",
        doc: "checks that no init functions are present in Go code",
        url: "https://github.com/leighmcculloch/gochecknoinits",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
