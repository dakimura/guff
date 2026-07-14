//! Port of [`github.com/alexkohler/nakedret`](https://github.com/alexkohler/nakedret)
//! (golangci-lint wrapper in `pkg/golinters/nakedret`).
//!
//! Default matches golangci-lint: `max-func-lines=30`.
//!
//! DEFERRED: `linters.settings.nakedret` wiring (`max-func-lines`,
//! `skip-test-files`); SuggestedFix for explicit named returns.

use std::sync::OnceLock;

use guff::ast::{FuncType, ReturnStmt};
use guff::position::FileSet;
use guff::walk::{self, NodeRef, Visitor};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

/// golangci-lint default for `linters.settings.nakedret.max-func-lines`.
const MAX_FUNC_LINES: usize = 30;

struct FuncInfo {
    name: String,
    func_length: usize,
    report_naked: bool,
}

struct ReturnsVisitor<'a> {
    fset: &'a FileSet,
    functions: Vec<FuncInfo>,
    pending: &'a mut Vec<(u32, String)>,
}

fn has_named_returns(func_type: &FuncType) -> bool {
    let Some(results) = &func_type.results else {
        return false;
    };
    results
        .list
        .iter()
        .any(|field| !field.names.is_empty())
}

fn nested_func_name(functions: &[FuncInfo]) -> String {
    functions
        .iter()
        .map(|f| f.name.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

fn push_func(
    v: &mut ReturnsVisitor<'_>,
    name: String,
    func_type: &FuncType,
    start: guff::position::Pos,
    end: guff::position::Pos,
) {
    let start_line = v.fset.position(start).line;
    let end_line = v.fset.position(end).line;
    let mut length = end_line.saturating_sub(start_line) as usize;
    if length == 0 {
        length = 1;
    }
    v.functions.push(FuncInfo {
        name,
        func_length: length,
        report_naked: length > MAX_FUNC_LINES && has_named_returns(func_type),
    });
}

impl<'a> Visitor<'a> for ReturnsVisitor<'a> {
    fn enter(&mut self, node: NodeRef<'a>) -> bool {
        match node {
            NodeRef::FuncDecl(f) => {
                let start = f.ty.pos();
                let end = f
                    .body
                    .as_ref()
                    .map(|b| b.end())
                    .unwrap_or_else(|| f.ty.end());
                push_func(self, f.name.name.clone(), &f.ty, start, end);
            }
            NodeRef::FuncLit(lit) => {
                let start = lit.ty.pos();
                let end = lit.body.end();
                let line = self.fset.position(start).line;
                push_func(self, format!("<func():{line}>"), &lit.ty, start, end);
            }
            NodeRef::ReturnStmt(ret) => {
                check_return(self, ret);
            }
            _ => {}
        }
        true
    }

    fn leave(&mut self, node: NodeRef<'a>) {
        if matches!(node, NodeRef::FuncDecl(_) | NodeRef::FuncLit(_)) {
            self.functions.pop();
        }
    }
}

fn check_return(v: &mut ReturnsVisitor<'_>, ret: &ReturnStmt) {
    let Some(fun) = v.functions.last() else {
        return;
    };
    if !fun.report_naked || !ret.results.is_empty() {
        return;
    }
    let fun_name = nested_func_name(&v.functions);
    let length = fun.func_length;
    v.pending.push((
        ret.return_.0 as u32,
        format!("naked return in func `{fun_name}` with {length} lines of code"),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "nakedret requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    let fset = pass.fset().clone();
    for file in pass.files() {
        let mut visitor = ReturnsVisitor {
            fset: &fset,
            functions: Vec::new(),
            pending: &mut pending,
        };
        walk::walk(&mut visitor, NodeRef::File(file));
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "nakedret",
        doc: "Checks that functions with naked returns are not longer than a maximum size (can be zero).",
        url: "https://github.com/alexkohler/nakedret",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
