//! Port of [`github.com/alexkohler/nakedret`](https://github.com/alexkohler/nakedret)
//! (golangci-lint wrapper in `pkg/golinters/nakedret`).
//!
//! Default matches golangci-lint: `max-func-lines=30`.
//!
//! The fix replaces the naked `return` with the function's result names,
//! rendered in declaration order (`nakedret.go:227 nakedReturnFix`).

use std::sync::OnceLock;

use guff::ast::{FuncType, ReturnStmt};
use guff::position::FileSet;
use guff::walk::{self, NodeRef, Visitor};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::options::NakedretOptions;

struct FuncInfo {
    name: String,
    func_length: usize,
    report_naked: bool,
    /// Every result name, flattened in declaration order.
    ///
    /// Upstream loops `Results.List` **and then** `result.Names`
    /// (`nakedret.go:229`), so a grouped `(a, b int)` — one field, two names —
    /// contributes both. Iterating fields alone renders `return a` and drops
    /// `b` silently, which is why the fixture gained a grouped case
    /// (COMPAT-HARDENING 続き 79).
    result_names: Vec<String>,
}

/// `nakedReturnFix`: the result names, in order. Only `nil` idents are skipped
/// upstream, so a blank `_` result name is carried through as written.
fn result_names(func_type: &FuncType) -> Vec<String> {
    let Some(results) = &func_type.results else {
        return Vec::new();
    };
    results
        .list
        .iter()
        .flat_map(|field| field.names.iter().map(|n| n.name.clone()))
        .collect()
}

struct ReturnsVisitor<'a> {
    fset: &'a FileSet,
    max_func_lines: usize,
    skip_test_files: bool,
    current_file: String,
    functions: Vec<FuncInfo>,
    pending: &'a mut Vec<(u32, String, Option<(u32, u32, String)>)>,
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
    max_func_lines: usize,
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
        report_naked: length > max_func_lines && has_named_returns(func_type),
        result_names: result_names(func_type),
    });
}

impl<'a> Visitor<'a> for ReturnsVisitor<'a> {
    fn enter(&mut self, node: NodeRef<'a>) -> bool {
        match node {
            NodeRef::FuncDecl(f) => {
                if self.skip_test_files && self.current_file.ends_with("_test.go") {
                    return false;
                }
                let start = f.ty.pos();
                let end = f
                    .body
                    .as_ref()
                    .map(|b| b.end())
                    .unwrap_or_else(|| f.ty.end());
                push_func(
                    self,
                    f.name.name.clone(),
                    &f.ty,
                    start,
                    end,
                    self.max_func_lines,
                );
            }
            NodeRef::FuncLit(lit) => {
                if self.skip_test_files && self.current_file.ends_with("_test.go") {
                    return false;
                }
                let start = lit.ty.pos();
                let end = lit.body.end();
                let line = self.fset.position(start).line;
                push_func(
                    self,
                    format!("<func():{line}>"),
                    &lit.ty,
                    start,
                    end,
                    self.max_func_lines,
                );
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
    let names = fun.result_names.join(", ");
    let fun_name = nested_func_name(&v.functions);
    let length = fun.func_length;
    // `Pos: s.Pos(), End: s.End()` — the whole `return`, replaced by the
    // explicit form. go/ast's `ReturnStmt.End()` is `Return + len("return")`
    // when there are no results, and the guard above is exactly that case.
    let end = ret.return_.0 as u32 + "return".len() as u32;
    v.pending.push((
        ret.return_.0 as u32,
        format!("naked return in func `{fun_name}` with {length} lines of code"),
        Some((
            ret.return_.0 as u32,
            end,
            format!("return {names}"),
        )),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "nakedret requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<NakedretOptions>("nakedret")
        .cloned()
        .unwrap_or_default();
    let max_func_lines = options.max_func_lines;
    let skip_test_files = options.skip_test_files;

    let mut pending = Vec::new();
    let fset = pass.fset().clone();
    let pkg = pass.pkg();
    for (i, file) in pass.files().iter().enumerate() {
        let fallback = fset.position(file.pos()).filename;
        let filename = pkg
            .compiled_go_files
            .get(i)
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or(fallback.as_str())
            .to_string();
        let mut visitor = ReturnsVisitor {
            fset: &fset,
            max_func_lines,
            skip_test_files,
            current_file: filename,
            functions: Vec::new(),
            pending: &mut pending,
        };
        walk::walk(&mut visitor, NodeRef::File(file));
    }

    for (pos, message, fix) in pending {
        let Some((from, to, new_text)) = fix else {
            pass.reportf(pos, message);
            continue;
        };
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "explicit return statement".into(),
                text_edits: vec![TextEdit {
                    pos: from,
                    end: to,
                    new_text,
                }],
            }],
            ..Diagnostic::default()
        });
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
        // AST-only (function length + naked `return`); still useful when the
        // package is ill-typed so `//nolint:nakedret` is marked used.
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
