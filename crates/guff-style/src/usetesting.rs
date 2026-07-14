//! Port of [`github.com/ldez/usetesting`](https://github.com/ldez/usetesting).
//!
//! Defaults match upstream: `os.MkdirTemp` / `os.CreateTemp("", …)` / `os.Chdir`
//! (Go ≥ 1.24) on; `context.Background` / `context.TODO` / `os.Setenv` /
//! `os.TempDir` off.
//!
//! DEFERRED: per-check flags via `linters.settings.usetesting`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, Field, FuncType, Ident};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

struct FuncInfo {
    name: String,
    arg_name: String,
}

fn sel_pkg_and_name(expr: &Expr) -> Option<(&str, &str)> {
    let Expr::SelectorExpr(se) = expr else {
        return None;
    };
    let Expr::Ident(pkg) = se.x.as_ref() else {
        return None;
    };
    Some((pkg.name.as_str(), se.sel.name.as_str()))
}

fn check_selector_name(se_x: &Ident, se_sel: &Ident, pkg: &str, names: &[&str]) -> bool {
    se_x.name == pkg && names.iter().any(|n| se_sel.name == *n)
}

fn test_arg_name(field: &Field, default: &str) -> String {
    if let Some(id) = field.names.first() {
        if id.name != "_" {
            return id.name.clone();
        }
    }
    default.to_string()
}

fn check_test_signature(field: &Field, fn_name: &str) -> Option<FuncInfo> {
    let ty = field.ty.as_ref()?;
    match ty {
        Expr::StarExpr(star) => {
            let Expr::SelectorExpr(se) = star.x.as_ref() else {
                return None;
            };
            let Expr::Ident(pkg) = se.x.as_ref() else {
                return None;
            };
            if !check_selector_name(pkg, &se.sel, "testing", &["T", "B"]) {
                return None;
            }
            Some(FuncInfo {
                name: fn_name.to_string(),
                arg_name: test_arg_name(field, "<t/b>"),
            })
        }
        Expr::SelectorExpr(se) => {
            let Expr::Ident(pkg) = se.x.as_ref() else {
                return None;
            };
            if !check_selector_name(pkg, &se.sel, "testing", &["TB"]) {
                return None;
            }
            Some(FuncInfo {
                name: fn_name.to_string(),
                arg_name: test_arg_name(field, "tb"),
            })
        }
        _ => None,
    }
}

fn first_param_field(ty: &FuncType) -> Option<&Field> {
    ty.params.as_ref()?.list.first()
}

fn is_empty_string_lit(expr: &Expr) -> bool {
    match expr {
        Expr::BasicLit(lit) => lit.kind == Some(Token::STRING) && lit.value == "\"\"",
        _ => false,
    }
}

fn go_ge_124(pass: &Pass<'_>) -> bool {
    code::version_compare(&code::module_go_version(pass), "go1.24") >= 0
}

fn check_call(
    call: &CallExpr,
    fn_info: &FuncInfo,
    ge_go124: bool,
    pending: &mut Vec<(u32, String)>,
) {
    // os.CreateTemp("", …) → t.TempDir()
    if let Some(("os", "CreateTemp")) = sel_pkg_and_name(&call.fun) {
        if call.args.len() == 2 && is_empty_string_lit(&call.args[0]) {
            pending.push((
                call.pos().0 as u32,
                format!(
                    "os.CreateTemp(\"\", ...) could be replaced by {}.TempDir() in {}",
                    fn_info.arg_name, fn_info.name
                ),
            ));
        }
        return;
    }

    let Some((pkg, name)) = sel_pkg_and_name(&call.fun) else {
        return;
    };

    let replacement = match (pkg, name) {
        ("os", "MkdirTemp") => Some("TempDir"),
        // Defaults off — keep disabled until settings wiring (DEFERRED).
        ("os", "TempDir") | ("os", "Setenv") => None,
        ("os", "Chdir") if ge_go124 => Some("Chdir"),
        // Defaults off for context.*
        ("context", "Background") | ("context", "TODO") => None,
        _ => None,
    };

    if let Some(expect) = replacement {
        pending.push((
            call.fun.pos().0 as u32,
            format!(
                "{pkg}.{name}() could be replaced by {}.{expect}() in {}",
                fn_info.arg_name, fn_info.name
            ),
        ));
    }
}

fn check_func_body(
    body: &guff::ast::BlockStmt,
    fn_info: &FuncInfo,
    ge_go124: bool,
    pending: &mut Vec<(u32, String)>,
) {
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        match n {
            // Nested functions are handled when visited as their own FuncDecl/FuncLit.
            NodeRef::FuncLit(_) | NodeRef::FuncDecl(_) => false,
            NodeRef::CallExpr(call) => {
                check_call(call, fn_info, ge_go124, pending);
                true
            }
            _ => true,
        }
    });
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "usetesting requires inspect analyzer".to_string())?;

    let ge_go124 = go_ge_124(pass);
    let mut pending = Vec::new();

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
                    let Some(field) = first_param_field(&fd.ty) else {
                        return true;
                    };
                    let Some(info) = check_test_signature(field, fd.name.name.as_str()) else {
                        return true;
                    };
                    check_func_body(body, &info, ge_go124, &mut pending);
                    true
                }
                NodeRef::FuncLit(fl) => {
                    let Some(field) = first_param_field(&fl.ty) else {
                        return true;
                    };
                    let Some(info) = check_test_signature(field, "anonymous function") else {
                        return true;
                    };
                    check_func_body(&fl.body, &info, ge_go124, &mut pending);
                    true
                }
                _ => true,
            }
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
        name: "usetesting",
        doc: "Reports uses of functions with replacement inside the testing package.",
        url: "https://github.com/ldez/usetesting",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
