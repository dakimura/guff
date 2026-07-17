//! Port of [`github.com/moricho/tparallel`](https://github.com/moricho/tparallel)
//! (golangci-lint wrapper in `pkg/golinters/tparallel`).
//!
//! Detects inappropriate usage of `t.Parallel()` in Go tests that have
//! subtests (`t.Run`):
//! - top-level vs subtest parallelization mismatch
//! - `defer` used instead of `t.Cleanup` when subtests call `t.Parallel`
//!
//! AST approximation of the upstream SSA-based analyzer. Helper functions that
//! themselves call `t.Run` / `t.Parallel` (indirect via `*testing.T` argument)
//! and local-variable callbacks (`fn := helper; t.Run(..., fn)`) are DEFERRED
//! (→ R17 / deeper assignment tracking).

use std::sync::OnceLock;

use guff::ast::{CallExpr, Decl, Expr, FuncDecl};
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

struct PendingReport {
    pos: u32,
    message: String,
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

fn node_call_has_method(n: NodeRef<'_>, receiver_name: &str, method_name: &str) -> bool {
    match n {
        NodeRef::CallExpr(call) => call_has_method(call, receiver_name, method_name),
        NodeRef::ExprStmt(es) => {
            if let Expr::CallExpr(call) = &es.x {
                call_has_method(call, receiver_name, method_name)
            } else {
                false
            }
        }
        _ => false,
    }
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

fn func_decl_calls_method(func_decl: &FuncDecl, method: &str) -> bool {
    let Some(test_var) = is_function_receiving_test_context(func_decl) else {
        return false;
    };
    let mut found = false;
    // Always continue walking: guff's `preorder` stops the *entire* walk on
    // the first `false` (unlike Go ast.Inspect subtree skip).
    walk::preorder(NodeRef::FuncDecl(func_decl), |n| {
        if !found {
            found = node_call_has_method(n, &test_var, method);
        }
        true
    });
    found
}

fn lookup_package_func<'a>(pass: &'a Pass<'_>, name: &str) -> Option<&'a FuncDecl> {
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            if fd.recv.is_none() && fd.name.name == name {
                return Some(fd);
            }
        }
    }
    None
}

/// Whether a `t.Run` callback (arg 1) calls `t.Parallel`.
fn run_callback_has_parallel(pass: &Pass<'_>, call: &CallExpr) -> bool {
    if call.args.len() < 2 {
        return false;
    }
    match &call.args[1] {
        Expr::FuncLit(func_lit) => {
            let Some(params) = func_lit.ty.params.as_ref() else {
                return false;
            };
            let Some(first) = params.list.first() else {
                return false;
            };
            let Some(test_var) = first.names.first().map(|n| n.name.as_str()) else {
                return false;
            };
            let mut found = false;
            walk::preorder(NodeRef::FuncLit(func_lit), |n| {
                if !found {
                    found = node_call_has_method(n, test_var, "Parallel");
                }
                true
            });
            found
        }
        Expr::Ident(ident) => lookup_package_func(pass, &ident.name)
            .map(|fd| func_decl_calls_method(fd, "Parallel"))
            .unwrap_or(false),
        // DEFERRED: local vars (`fn := helper`) and builder CallExpr returns.
        _ => false,
    }
}

fn stack_inside_func_lit(stack: &[NodeRef<'_>]) -> bool {
    stack.iter().any(|n| matches!(n, NodeRef::FuncLit(_)))
}

#[derive(Default)]
struct Analysis {
    has_subtests: bool,
    is_parallel_top: bool,
    is_parallel_sub: bool,
    has_defer: bool,
    has_cleanup: bool,
}

fn analyze_test_function(pass: &Pass<'_>, func_decl: &FuncDecl, reports: &mut Vec<PendingReport>) {
    let Some(test_var) = is_test_function(func_decl) else {
        return;
    };
    let Some(body) = func_decl.body.as_ref() else {
        return;
    };

    let mut analysis = Analysis::default();
    let mut stack: Vec<NodeRef<'_>> = Vec::new();

    // Use preorder_stack so returning false only skips a subtree (unlike
    // walk::preorder, which aborts the entire remaining walk).
    walk::preorder_stack(NodeRef::BlockStmt(body), &mut stack, |n, stack| {
        match n {
            NodeRef::DeferStmt(_) => {
                if !stack_inside_func_lit(stack) {
                    analysis.has_defer = true;
                }
                true
            }
            NodeRef::CallExpr(call) => {
                let in_lit = stack_inside_func_lit(stack);
                if !in_lit {
                    if call_has_method(call, &test_var, "Parallel") {
                        analysis.is_parallel_top = true;
                    }
                    if call_has_method(call, &test_var, "Cleanup") {
                        analysis.has_cleanup = true;
                    }
                }
                if call_has_method(call, &test_var, "Run") {
                    // Only count t.Run on the test var at top-level (not
                    // nested inside an unrelated FuncLit).
                    if !in_lit {
                        analysis.has_subtests = true;
                        if run_callback_has_parallel(pass, call) {
                            analysis.is_parallel_sub = true;
                        }
                    }
                    // Skip walking into Run args so Parallel inside the
                    // callback FuncLit is not counted as top-level.
                    false
                } else {
                    true
                }
            }
            NodeRef::FuncLit(_) => {
                // Skip unrelated FuncLits (e.g. Cleanup callbacks): their
                // Parallel must not count as top-level.
                false
            }
            _ => true,
        }
    });

    if !analysis.has_subtests {
        return;
    }

    let func_name = &func_decl.name.name;
    let func_pos = func_decl.name.pos().0 as u32;

    if analysis.has_defer && analysis.is_parallel_sub && !analysis.has_cleanup {
        reports.push(PendingReport {
            pos: func_pos,
            message: format!("{func_name} should use t.Cleanup instead of defer"),
        });
    }

    if analysis.is_parallel_top == analysis.is_parallel_sub {
        return;
    }
    if analysis.is_parallel_sub {
        reports.push(PendingReport {
            pos: func_pos,
            message: format!(
                "{func_name} should call t.Parallel on the top level as well as its subtests"
            ),
        });
    } else {
        reports.push(PendingReport {
            pos: func_pos,
            message: format!("{func_name}'s subtests should call t.Parallel"),
        });
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "tparallel requires inspect analyzer".to_string())?;

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
            analyze_test_function(pass, fd, &mut reports);
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
        name: "tparallel",
        doc: "tparallel detects inappropriate usage of t.Parallel() method in your Go test codes",
        url: "https://github.com/moricho/tparallel",
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
