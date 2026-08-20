//! Port of [`github.com/kulti/thelper`](https://github.com/kulti/thelper)
//! (golangci-lint wrapper in `pkg/golinters/thelper`).
//!
//! Detects test helpers that omit `t.Helper()` / wrong param name / wrong param
//! order. Subtests passed only to `t.Run` / `b.Run` / `f.Fuzz` are filtered out
//! (unless also referenced elsewhere).
//!
//! DEFERRED: full unwrap of subtest builders that return
//! `func(*testing.T)`, and Selections-based method identity (AST name + receiver
//! type heuristics are used instead).

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{BlockStmt, CallExpr, Expr, Field, FuncType, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::ObjectData;

use crate::options::{ThelperKindOptions, ThelperOptions};

#[derive(Clone, Copy)]
enum HelperKind {
    T,
    F,
    B,
    Tb,
}

impl HelperKind {
    fn skip_prefix(self) -> &'static str {
        match self {
            Self::T => "Test",
            Self::F => "Fuzz",
            Self::B => "Benchmark",
            Self::Tb => "",
        }
    }

    fn var_name(self) -> &'static str {
        match self {
            Self::T => "t",
            Self::F => "f",
            Self::B => "b",
            Self::Tb => "tb",
        }
    }

    fn type_label(self) -> &'static str {
        match self {
            Self::T => "*testing.T",
            Self::F => "*testing.F",
            Self::B => "*testing.B",
            Self::Tb => "testing.TB",
        }
    }

    fn opts(self, all: &ThelperOptions) -> &ThelperKindOptions {
        match self {
            Self::T => &all.test,
            Self::F => &all.fuzz,
            Self::B => &all.benchmark,
            Self::Tb => &all.tb,
        }
    }
}

struct PendingReport {
    pos: u32,
    message: String,
}

struct Reports {
    reports: Vec<PendingReport>,
    filter: HashSet<u32>,
    nofilter: HashSet<u32>,
}

impl Reports {
    fn new() -> Self {
        Self {
            reports: Vec::new(),
            filter: HashSet::new(),
            nofilter: HashSet::new(),
        }
    }

    fn reportf(&mut self, pos: u32, message: String) {
        self.reports.push(PendingReport { pos, message });
    }

    fn filter(&mut self, pos: u32) {
        if pos != 0 {
            self.filter.insert(pos);
        }
    }

    fn nofilter(&mut self, pos: u32) {
        if pos != 0 {
            self.nofilter.insert(pos);
        }
    }

    fn flush(self, pass: &mut Pass<'_>) {
        for r in self.reports {
            if self.filter.contains(&r.pos) && !self.nofilter.contains(&r.pos) {
                continue;
            }
            pass.reportf(r.pos, r.message);
        }
    }
}

struct FuncView<'a> {
    report_pos: u32,
    name: &'a str,
    ty: &'a FuncType,
    body: Option<&'a BlockStmt>,
}

fn is_testing_star(expr: &Expr, type_name: &str) -> bool {
    let Expr::StarExpr(star) = expr else {
        return false;
    };
    let Expr::SelectorExpr(se) = star.x.as_ref() else {
        return false;
    };
    let Expr::Ident(pkg) = se.x.as_ref() else {
        return false;
    };
    pkg.name == "testing" && se.sel.name == type_name
}

fn is_testing_tb(expr: &Expr) -> bool {
    let Expr::SelectorExpr(se) = expr else {
        return false;
    };
    let Expr::Ident(pkg) = se.x.as_ref() else {
        return false;
    };
    pkg.name == "testing" && se.sel.name == "TB"
}

fn is_context_context(expr: &Expr) -> bool {
    let Expr::SelectorExpr(se) = expr else {
        return false;
    };
    let Expr::Ident(pkg) = se.x.as_ref() else {
        return false;
    };
    pkg.name == "context" && se.sel.name == "Context"
}

fn matches_helper_type(expr: &Expr, kind: HelperKind) -> bool {
    match kind {
        HelperKind::T => is_testing_star(expr, "T"),
        HelperKind::F => is_testing_star(expr, "F"),
        HelperKind::B => is_testing_star(expr, "B"),
        HelperKind::Tb => is_testing_tb(expr),
    }
}

fn search_func_param<'a>(
    ty: &'a FuncType,
    kind: HelperKind,
) -> Option<(&'a Field, usize)> {
    let params = ty.params.as_ref()?;
    for (i, field) in params.list.iter().enumerate() {
        let Some(ft) = field.ty.as_ref() else {
            continue;
        };
        if matches_helper_type(ft, kind) {
            return Some((field, i));
        }
    }
    None
}

fn is_helper_call(stmt: &Stmt, param_name: &str) -> bool {
    let Stmt::ExprStmt(es) = stmt else {
        return false;
    };
    let Expr::CallExpr(call) = &es.x else {
        return false;
    };
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return false;
    };
    if sel.sel.name != "Helper" {
        return false;
    }
    // Upstream matches via Selections on the Helper method; we accept
    // `<param>.Helper()` by AST (covers renamed params when `name` is off).
    match sel.x.as_ref() {
        Expr::Ident(id) => id.name == param_name,
        _ => false,
    }
}

fn check_func(reports: &mut Reports, fd: &FuncView<'_>, kind: HelperKind, opts: &ThelperKindOptions) {
    if !opts.first && !opts.begin && !opts.name {
        return;
    }

    let skip = kind.skip_prefix();
    if !skip.is_empty() && fd.name.starts_with(skip) {
        return;
    }

    let Some((field, pos)) = search_func_param(fd.ty, kind) else {
        return;
    };

    if opts.first && pos != 0 {
        let mut ok = false;
        if pos == 1 {
            if let Some(params) = fd.ty.params.as_ref() {
                if let Some(first) = params.list.first() {
                    if let Some(ft) = first.ty.as_ref() {
                        ok = is_context_context(ft);
                    }
                }
            }
        }
        if !ok {
            reports.reportf(
                fd.report_pos,
                format!(
                    "parameter {} should be the first or after context.Context",
                    kind.type_label()
                ),
            );
        }
    }

    let named = field
        .names
        .first()
        .map(|n| n.name.as_str())
        .filter(|n| *n != "_");

    let Some(pname) = named else {
        return;
    };

    if opts.name && pname != kind.var_name() {
        reports.reportf(
            fd.report_pos,
            format!(
                "parameter {} should have name {}",
                kind.type_label(),
                kind.var_name()
            ),
        );
    }

    if opts.begin {
        let starts_with_helper = fd
            .body
            .and_then(|b| b.list.first())
            .is_some_and(|s| is_helper_call(s, pname));
        if !starts_with_helper {
            reports.reportf(
                fd.report_pos,
                format!(
                    "test helper function should start from {}.Helper()",
                    kind.var_name()
                ),
            );
        }
    }
}

fn is_run_or_fuzz_call(call: &CallExpr) -> Option<&'static str> {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return None;
    };
    match sel.sel.name.as_str() {
        "Run" => Some("Run"),
        "Fuzz" => Some("Fuzz"),
        _ => None,
    }
}

fn func_def_position(pass: &Pass<'_>, expr: &Expr) -> u32 {
    match expr {
        Expr::FuncLit(fl) => fl.ty.pos().0 as u32,
        Expr::Ident(id) => {
            let Some(info) = pass.types_info() else {
                return 0;
            };
            let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                return 0;
            };
            if let Some(&obj) = info.uses.get(&id.id) {
                return obj.pos(&artifacts.objects);
            }
            0
        }
        Expr::SelectorExpr(sel) => {
            let Some(info) = pass.types_info() else {
                return 0;
            };
            let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                return 0;
            };
            if let Some(&obj) = info.uses.get(&sel.sel.id) {
                return obj.pos(&artifacts.objects);
            }
            0
        }
        _ => 0,
    }
}

/// `synctest.Test(t, func(*testing.T) { … })` — upstream's
/// `extractSynctestExp`, which filters the literal exactly as `t.Run` does.
///
/// The identifier has to name the `testing/synctest` package: a local variable
/// called `synctest` with a `Test` method is not this.
fn extract_synctest_arg<'a>(pass: &Pass<'_>, call: &'a CallExpr) -> Option<&'a Expr> {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return None;
    };
    if sel.sel.name != "Test" || call.args.len() != 2 {
        return None;
    }
    let Expr::Ident(pkg) = sel.x.as_ref() else {
        return None;
    };
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj = info.uses.get(&pkg.id).copied()?;
    let ObjectData::PkgName(pn) = artifacts.objects.get(obj) else {
        return None;
    };
    if artifacts.packages.get(pn.imported()).path() != "testing/synctest" {
        return None;
    }
    Some(&call.args[1])
}

fn extract_subtest_arg<'a>(call: &'a CallExpr, method: &str) -> Option<&'a Expr> {
    match method {
        "Run" if call.args.len() == 2 => Some(&call.args[1]),
        "Fuzz" if call.args.len() == 1 => Some(&call.args[0]),
        _ => None,
    }
}

fn collect_builder_returns(body: &BlockStmt, out: &mut Vec<u32>) {
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::ReturnStmt(ret) = n {
            if ret.results.len() == 1 {
                if let Expr::FuncLit(fl) = &ret.results[0] {
                    out.push(fl.ty.pos().0 as u32);
                }
            }
        }
        true
    });
}

/// Best-effort unwrap of `t.Run(name, builder(...))` where builder returns
/// `func(*testing.T)`. DEFERRED full type check of return signature.
fn unwrap_builder_funcs(pass: &Pass<'_>, expr: &Expr) -> Vec<u32> {
    let Expr::CallExpr(call) = expr else {
        return Vec::new();
    };

    let mut bodies: Vec<&BlockStmt> = Vec::new();
    match call.fun.as_ref() {
        Expr::FuncLit(fl) => bodies.push(&fl.body),
        Expr::Ident(id) => {
            // Resolve via Uses → FuncDecl body in the same package.
            let Some(info) = pass.types_info() else {
                return Vec::new();
            };
            let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                return Vec::new();
            };
            let Some(&obj) = info.uses.get(&id.id) else {
                return Vec::new();
            };
            let want_pos = obj.pos(&artifacts.objects);
            for file in pass.files() {
                for decl in &file.decls {
                    if let guff::ast::Decl::FuncDecl(fd) = decl {
                        if fd.name.pos().0 as u32 == want_pos {
                            if let Some(body) = &fd.body {
                                bodies.push(body);
                            }
                        }
                    }
                }
            }
        }
        _ => return Vec::new(),
    }

    let mut out = Vec::new();
    for body in bodies {
        collect_builder_returns(body, &mut out);
    }
    out
}

fn handle_call(pass: &Pass<'_>, call: &CallExpr, reports: &mut Reports) {
    let subtest_arg = match is_run_or_fuzz_call(call) {
        Some(method) => extract_subtest_arg(call, method),
        None => None,
    }
    .or_else(|| extract_synctest_arg(pass, call));
    {
        if let Some(arg) = subtest_arg {
            let mut filtered = false;
            let builder_pos = unwrap_builder_funcs(pass, arg);
            if !builder_pos.is_empty() {
                for p in builder_pos {
                    reports.filter(p);
                    filtered = true;
                }
            }
            let pos = func_def_position(pass, arg);
            if pos != 0 {
                reports.filter(pos);
                filtered = true;
            }
            if filtered {
                return;
            }
        }
    }

    // Any other call referencing a function → mark nofilter (used as helper).
    reports.nofilter(func_def_position(pass, &call.fun));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "thelper requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<ThelperOptions>("thelper")
        .cloned()
        .unwrap_or_default();

    let mut reports = Reports::new();
    let kinds = [
        HelperKind::T,
        HelperKind::F,
        HelperKind::B,
        HelperKind::Tb,
    ];

    // Walk once: collect checks + call sites for filter.
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::FuncDecl(fd) => {
                    let Some(body) = fd.body.as_ref() else {
                        return true;
                    };
                    let view = FuncView {
                        report_pos: fd.name.pos().0 as u32,
                        name: fd.name.name.as_str(),
                        ty: &fd.ty,
                        body: Some(body),
                    };
                    for kind in kinds {
                        check_func(&mut reports, &view, kind, kind.opts(&opts));
                    }
                }
                NodeRef::FuncLit(fl) => {
                    let view = FuncView {
                        report_pos: fl.ty.pos().0 as u32,
                        name: "",
                        ty: &fl.ty,
                        body: Some(&fl.body),
                    };
                    for kind in kinds {
                        check_func(&mut reports, &view, kind, kind.opts(&opts));
                    }
                }
                NodeRef::CallExpr(call) => {
                    handle_call(pass, call, &mut reports);
                }
                _ => {}
            }
            true
        });
    }

    reports.flush(pass);
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "thelper",
        doc: "detects golang test helpers without t.Helper() call and checks the consistency of test helpers",
        url: "https://github.com/kulti/thelper",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::ThelperKindOptions;

    #[test]
    fn default_kind_all_enabled() {
        let k = ThelperKindOptions::default();
        assert!(k.first && k.name && k.begin);
    }

    #[test]
    fn matches_star_t() {
        // Smoke: helpers used by unit tests via type helpers.
        assert_eq!(HelperKind::T.var_name(), "t");
        assert_eq!(HelperKind::Tb.type_label(), "testing.TB");
    }
}
