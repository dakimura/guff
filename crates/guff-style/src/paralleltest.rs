//! Port of [`github.com/kunwardeep/paralleltest`](https://github.com/kunwardeep/paralleltest)
//! (golangci-lint wrapper in `pkg/golinters/paralleltest`).
//!
//! Detects missing `t.Parallel()` in `Test*` functions and their `t.Run`
//! subtests. Also optionally flags `defer` when used together with
//! `t.Parallel` (`check-cleanup`).
//!
//! Settings (`linters.settings.paralleltest`):
//! - `ignore-missing` (default false)
//! - `ignore-missing-subtests` (default false)
//! - `check-cleanup` (default false)
//!
//! DEFERRED: loop-variable reinit detection (`ignoreloopVar`; golangci enables
//! ignore for Go ≥ 1.22), builder-function returns used as `t.Run` callbacks.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Decl, Expr, FuncDecl, Stmt};
use guff::walk::{self, expr_ref, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::ParalleltestOptions;

struct PendingReport {
    pos: u32,
    message: String,
}

#[derive(Default)]
struct TestRunAnalysis {
    has_parallel: bool,
    cant_parallel: bool,
    number_of_test_run: usize,
    position_of_test_run_node: Vec<u32>,
}

#[derive(Default)]
struct TestFunctionAnalysis {
    func_has_parallel_method: bool,
    func_cant_parallel_method: bool,
    range_statement_over_test_cases_exists: bool,
    range_statement_has_parallel_method: bool,
    range_statement_cant_parallel_method: bool,
    func_has_defer_statement: bool,
    number_of_test_run: usize,
    position_of_test_run_node: Vec<u32>,
    range_pos: Option<u32>,
    defer_positions: Vec<u32>,
}

fn call_has_method(call: &CallExpr, receiver_name: &str, method_name: &str) -> bool {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return false;
    };
    let Expr::Ident(recv) = sel.x.as_ref() else {
        return false;
    };
    recv.name == receiver_name && sel.sel.name == method_name
}

fn expr_call_has_method(node: &Expr, receiver_name: &str, method_name: &str) -> bool {
    match node {
        Expr::CallExpr(call) => call_has_method(call, receiver_name, method_name),
        _ => false,
    }
}

fn node_call_has_method(n: NodeRef<'_>, receiver_name: &str, method_name: &str) -> bool {
    match n {
        NodeRef::CallExpr(call) => call_has_method(call, receiver_name, method_name),
        NodeRef::ExprStmt(es) => expr_call_has_method(&es.x, receiver_name, method_name),
        _ => false,
    }
}

fn get_run_callback_parameter_name(call: &CallExpr) -> String {
    if call.args.len() < 2 {
        return String::new();
    }
    let Expr::FuncLit(fun) = &call.args[1] else {
        return String::new();
    };
    let Some(params) = fun.ty.params.as_ref() else {
        return String::new();
    };
    let Some(first) = params.list.first() else {
        return String::new();
    };
    first
        .names
        .first()
        .map(|n| n.name.clone())
        .unwrap_or_default()
}

fn is_star_testing_t(expr: &Expr) -> bool {
    let Expr::StarExpr(star) = expr else {
        return false;
    };
    let Expr::SelectorExpr(se) = star.x.as_ref() else {
        return false;
    };
    let Expr::Ident(pkg) = se.x.as_ref() else {
        return false;
    };
    pkg.name == "testing" && se.sel.name == "T"
}

fn is_test_function(func_decl: &FuncDecl) -> Option<String> {
    if !func_decl.name.name.starts_with("Test") {
        return None;
    }
    let params = func_decl.ty.params.as_ref()?;
    if params.list.len() != 1 {
        return None;
    }
    let param = &params.list[0];
    let ty = param.ty.as_ref()?;
    if !is_star_testing_t(ty) {
        return None;
    }
    param.names.first().map(|n| n.name.clone())
}

fn is_function_receiving_test_context(func_decl: &FuncDecl) -> Option<String> {
    let params = func_decl.ty.params.as_ref()?;
    if params.list.len() != 1 {
        return None;
    }
    let param = &params.list[0];
    let ty = param.ty.as_ref()?;
    if !is_star_testing_t(ty) {
        return None;
    }
    param.names.first().map(|n| n.name.clone())
}

fn target_method_in_method_run(call: &CallExpr, test_var: &str, target: &str) -> bool {
    let mut called = false;
    for arg in &call.args {
        walk::preorder(expr_ref(arg), |n| {
            if !called {
                called = node_call_has_method(n, test_var, target);
            }
            !called
        });
        if called {
            break;
        }
    }
    called
}

fn analyze_test_run(pass: &Pass<'_>, call: &CallExpr, test_var: &str) -> TestRunAnalysis {
    let mut analysis = TestRunAnalysis::default();
    if !call_has_method(call, test_var, "Run") {
        return analysis;
    }

    let inner_test_var = get_run_callback_parameter_name(call);
    analysis.number_of_test_run += 1;

    if call.args.len() > 1 {
        match &call.args[1] {
            Expr::FuncLit(func_lit) => {
                if !inner_test_var.is_empty() {
                    walk::preorder(NodeRef::FuncLit(func_lit), |p| {
                        if !analysis.has_parallel {
                            analysis.has_parallel =
                                node_call_has_method(p, &inner_test_var, "Parallel");
                        }
                        if !analysis.cant_parallel {
                            analysis.cant_parallel =
                                node_call_has_method(p, &inner_test_var, "Setenv");
                        }
                        true
                    });
                }
            }
            Expr::Ident(ident) => {
                let mut found_func = false;
                for file in pass.files() {
                    for decl in &file.decls {
                        let Decl::FuncDecl(fd) = decl else {
                            continue;
                        };
                        if fd.name.name != ident.name {
                            continue;
                        }
                        found_func = true;
                        if let Some(test_param_name) = is_function_receiving_test_context(fd) {
                            walk::preorder(NodeRef::FuncDecl(fd), |p| {
                                if !analysis.has_parallel {
                                    analysis.has_parallel =
                                        node_call_has_method(p, &test_param_name, "Parallel");
                                }
                                true
                            });
                        }
                    }
                }
                if !found_func {
                    analysis.has_parallel = false;
                }
            }
            Expr::CallExpr(_) => {
                // DEFERRED: builder functions that return a test func.
                analysis.has_parallel = false;
            }
            _ => {
                analysis.has_parallel = false;
            }
        }
    }

    if !analysis.has_parallel && !analysis.cant_parallel {
        analysis
            .position_of_test_run_node
            .push(call.pos().0 as u32);
    }

    analysis
}

fn analyze_test_function(
    pass: &Pass<'_>,
    func_decl: &FuncDecl,
    opts: &ParalleltestOptions,
    reports: &mut Vec<PendingReport>,
) {
    let Some(test_var) = is_test_function(func_decl) else {
        return;
    };
    let Some(body) = func_decl.body.as_ref() else {
        return;
    };

    let mut analysis = TestFunctionAnalysis::default();

    for stmt in &body.list {
        match stmt {
            Stmt::DeferStmt(d) => {
                if opts.check_cleanup {
                    analysis.func_has_defer_statement = true;
                    analysis.defer_positions.push(d.defer_.0 as u32);
                }
            }
            Stmt::ExprStmt(es) => {
                walk::preorder(NodeRef::ExprStmt(es), |n| {
                    if !analysis.func_has_parallel_method {
                        analysis.func_has_parallel_method =
                            node_call_has_method(n, &test_var, "Parallel");
                    }
                    if !analysis.func_cant_parallel_method {
                        analysis.func_cant_parallel_method =
                            node_call_has_method(n, &test_var, "Setenv");
                    }
                    if let NodeRef::CallExpr(call) = n {
                        if call_has_method(call, &test_var, "Run") {
                            let run = analyze_test_run(pass, call, &test_var);
                            analysis.number_of_test_run += run.number_of_test_run;
                            analysis
                                .position_of_test_run_node
                                .extend(run.position_of_test_run_node);
                        }
                    }
                    true
                });
            }
            Stmt::RangeStmt(range) => {
                analysis.range_pos = Some(range.for_.0 as u32);
                walk::preorder(NodeRef::RangeStmt(range), |n| {
                    let NodeRef::ExprStmt(r) = n else {
                        return true;
                    };
                    let Expr::CallExpr(call) = &r.x else {
                        return true;
                    };
                    if !call_has_method(call, &test_var, "Run") {
                        return true;
                    }
                    let inner_test_var = get_run_callback_parameter_name(call);
                    analysis.range_statement_over_test_cases_exists = true;

                    if !analysis.range_statement_has_parallel_method {
                        analysis.range_statement_has_parallel_method =
                            target_method_in_method_run(call, &inner_test_var, "Parallel");
                    }
                    if !analysis.range_statement_cant_parallel_method {
                        analysis.range_statement_cant_parallel_method =
                            target_method_in_method_run(call, &inner_test_var, "Setenv");
                    }

                    if call.args.len() > 1 {
                        if let Expr::FuncLit(func_lit) = &call.args[1] {
                            walk::preorder(NodeRef::FuncLit(func_lit), |p| {
                                if let NodeRef::CallExpr(nested) = p {
                                    if call_has_method(nested, &inner_test_var, "Run") {
                                        let run =
                                            analyze_test_run(pass, nested, &inner_test_var);
                                        analysis.number_of_test_run += run.number_of_test_run;
                                        analysis
                                            .position_of_test_run_node
                                            .extend(run.position_of_test_run_node);
                                    }
                                }
                                true
                            });
                        }
                    }
                    true
                });
            }
            _ => {}
        }
    }

    if analysis.range_statement_cant_parallel_method {
        analysis.func_cant_parallel_method = true;
    }

    let func_name = &func_decl.name.name;
    // `pass.Reportf(funcDecl.Pos(), …)` — the `func` keyword.
    //
    // Four of upstream's five messages end with a literal `\n`, which reaches
    // the user's terminal as a blank line. It looks like a typo and is not one
    // to us: the tiers that normalize strip trailing whitespace, so nothing but
    // the golden key can see it. The `t.Cleanup` message below is the one that
    // does *not* have it — copying the newline onto all five would be as wrong
    // as dropping it from all five.
    let func_pos = func_decl.ty.pos().0 as u32;

    if !opts.ignore_missing
        && !analysis.func_has_parallel_method
        && !analysis.func_cant_parallel_method
    {
        reports.push(PendingReport {
            pos: func_pos,
            message: format!("Function {func_name} missing the call to method parallel\n"),
        });
    }

    if analysis.range_statement_over_test_cases_exists {
        if let Some(range_pos) = analysis.range_pos {
            if !analysis.range_statement_has_parallel_method
                && !analysis.range_statement_cant_parallel_method
                && !opts.ignore_missing
                && !opts.ignore_missing_subtests
            {
                reports.push(PendingReport {
                    pos: range_pos,
                    message: format!(
                        "Range statement for test {func_name} missing the call to method parallel in test Run\n"
                    ),
                });
            }
            // DEFERRED: loop variable reinit (`ignoreloopVar`).
        }
    }

    if !opts.ignore_missing
        && !opts.ignore_missing_subtests
        && analysis.number_of_test_run > 1
        && !analysis.position_of_test_run_node.is_empty()
    {
        for pos in analysis.position_of_test_run_node {
            reports.push(PendingReport {
                pos,
                message: format!(
                    "Function {func_name} missing the call to method parallel in the test run\n"
                ),
            });
        }
    }

    if opts.check_cleanup
        && analysis.func_has_parallel_method
        && analysis.func_has_defer_statement
    {
        for pos in analysis.defer_positions {
            reports.push(PendingReport {
                pos,
                message: format!(
                    "Function {func_name} uses defer with t.Parallel, use t.Cleanup instead to ensure cleanup runs after parallel subtests complete"
                ),
            });
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "paralleltest requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<ParalleltestOptions>("paralleltest")
        .copied()
        .unwrap_or_default();

    let mut reports: Vec<PendingReport> = Vec::new();
    let pkg = pass.pkg();
    let fset = pass.fset();

    for (i, file) in pass.files().iter().enumerate() {
        let fallback = fset.position(file.pos()).filename;
        let filename = pkg
            .compiled_go_files
            .get(i)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| fallback.clone());
        if !filename.ends_with("_test.go") {
            continue;
        }
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            analyze_test_function(pass, fd, &opts, &mut reports);
        }
    }

    for r in reports {
        pass.reportf(r.pos, r.message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "paralleltest",
        doc: "Detects missing usage of t.Parallel() method in your Go test codes.",
        url: "https://github.com/kunwardeep/paralleltest",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn analyzer_graph_is_valid() {
        validate(&[analyzer()]).expect("valid analyzer graph");
    }
}
