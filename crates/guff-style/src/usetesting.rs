//! Port of [`github.com/ldez/usetesting`](https://github.com/ldez/usetesting).
//!
//! Defaults match upstream: `os.MkdirTemp` / `os.CreateTemp("", …)` / `os.Chdir`
//! (Go ≥ 1.24) on; `context.Background` / `context.TODO` / `os.Setenv` /
//! `os.TempDir` off.
//!
//! `linters.settings.usetesting` per-check flags are wired.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, Field, FuncType, Ident};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::options::UsetestingOptions;

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

/// Source-ish text for the pieces `os.CreateTemp`'s fix reassembles.
///
/// Upstream prints the rebuilt call with `printer.Fprint(buf,
/// token.NewFileSet(), g)` — a *fresh* FileSet, so the output is structural
/// rather than a slice of the original source. Only the shapes that actually
/// reach this fix are handled; anything else yields `None` and the finding is
/// reported without a fix, which under-fixes rather than writing a guess.
///
/// Deliberately local. `expr_text` exists in four other files in this crate
/// with four different jobs, and folding them together would replace a faithful
/// port with an approximation of a different one.
fn expr_text(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::BasicLit(lit) => Some(lit.value.clone()),
        Expr::SelectorExpr(sel) => Some(format!("{}.{}", expr_text(&sel.x)?, sel.sel.name)),
        _ => None,
    }
}

fn check_call(
    call: &CallExpr,
    fn_info: &FuncInfo,
    ge_go124: bool,
    options: &UsetestingOptions,
    pending: &mut Vec<(u32, String, Option<(u32, u32, String)>)>,
) {
    // os.CreateTemp("", …) → t.TempDir()
    if let Some(("os", "CreateTemp")) = sel_pkg_and_name(&call.fun) {
        if options.os_create_temp && call.args.len() == 2 && is_empty_string_lit(&call.args[0]) {
            // `diagnosticOSCreateTemp` rebuilds the call with the temp dir as
            // its first argument and replaces the whole `CallExpr`
            // (usetesting v0.5.0 `report.go:60`). The fix is skipped when the
            // test function's parameter is unnamed: `arg_name` is then the
            // placeholder `<t/b>`, and `<t/b>.TempDir()` is not Go.
            let fix = (!fn_info.arg_name.contains('<'))
                .then(|| {
                    let fun = expr_text(&call.fun)?;
                    let rest = expr_text(&call.args[1])?;
                    Some((
                        call.pos().0 as u32,
                        call.end().0 as u32,
                        format!("{fun}({}.TempDir(), {rest})", fn_info.arg_name),
                    ))
                })
                .flatten();
            pending.push((
                call.pos().0 as u32,
                format!(
                    // Alone among the arms, this one suggests replacing the
                    // *argument* rather than the call: `os.CreateTemp` still
                    // has to be called, just with a directory. The other six
                    // read `pkg.Name() could be replaced by t.Name()`, and
                    // sharing that shape here dropped the surrounding call.
                    "os.CreateTemp(\"\", ...) could be replaced by os.CreateTemp({}.TempDir(), ...) in {}",
                    fn_info.arg_name, fn_info.name
                ),
                fix,
            ));
        }
        return;
    }

    let Some((pkg, name)) = sel_pkg_and_name(&call.fun) else {
        return;
    };

    let replacement = match (pkg, name) {
        ("os", "MkdirTemp") if options.os_mkdir_temp => Some("TempDir"),
        ("os", "TempDir") if options.os_temp_dir => Some("TempDir"),
        ("os", "Setenv") if options.os_setenv => Some("Setenv"),
        ("os", "Chdir") if options.os_chdir && ge_go124 => Some("Chdir"),
        ("context", "Background") if options.context_background && ge_go124 => Some("Context"),
        ("context", "TODO") if options.context_todo && ge_go124 => Some("Context"),
        _ => None,
    };

    if let Some(expect) = replacement {
        // `report` attaches a fix only for `context.*` (report.go:159). Its
        // comment says the reason is matching return arity, which does not
        // hold — `os.TempDir()` and `t.TempDir()` have the same arity — but the
        // code is the specification, and rewriting the `os.*` arms would edit
        // calls upstream leaves alone.
        let fix = (pkg == "context" && !fn_info.arg_name.contains('<'))
            .then(|| {
                (
                    call.fun.pos().0 as u32,
                    call.fun.end().0 as u32,
                    format!("{}.{expect}", fn_info.arg_name),
                )
            });
        pending.push((
            call.fun.pos().0 as u32,
            format!(
                "{pkg}.{name}() could be replaced by {}.{expect}() in {}",
                fn_info.arg_name, fn_info.name
            ),
            fix,
        ));
    }
}

fn check_func_body(
    body: &guff::ast::BlockStmt,
    fn_info: &FuncInfo,
    ge_go124: bool,
    options: &UsetestingOptions,
    pending: &mut Vec<(u32, String, Option<(u32, u32, String)>)>,
) {
    // Upstream's `checkFunc` inspects the whole block, closures included, and
    // keeps the *enclosing* function's name and test-argument name in the
    // message. Stopping at a nested function instead meant a call inside a
    // closure was only ever attributed to the closure — and a closure with no
    // parameters has no test argument at all, so nothing was reported. gitea's
    // `testUploadAttachmentDeleteTemp` wraps its `os.TempDir()` in exactly such
    // a closure.
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::CallExpr(call) = n {
            check_call(call, fn_info, ge_go124, options, pending);
        }
        true
    });
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "usetesting requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<UsetestingOptions>("usetesting")
        .copied()
        .unwrap_or_default();

    if !options.os_create_temp
        && !options.os_mkdir_temp
        && !options.os_setenv
        && !options.os_temp_dir
        && !options.os_chdir
        && !options.context_background
        && !options.context_todo
    {
        return Ok(None);
    }

    let ge_go124 = go_ge_124(pass);
    let mut pending = Vec::new();

    for file in pass.files() {
        let mut stack: Vec<NodeRef<'_>> = Vec::new();
        walk::preorder_stack(NodeRef::File(file), &mut stack, |n, enclosing| {
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
                    check_func_body(body, &info, ge_go124, &options, &mut pending);
                }
                NodeRef::FuncLit(fl) => {
                    // `hasParentFunc`: a literal inside a function is already
                    // covered by that function's own walk, and reporting it
                    // again would name "anonymous function" where upstream
                    // names the enclosing one.
                    if enclosing
                        .iter()
                        .any(|p| matches!(p, NodeRef::FuncDecl(_) | NodeRef::FuncLit(_)))
                    {
                        return true;
                    }
                    let Some(field) = first_param_field(&fl.ty) else {
                        return true;
                    };
                    let Some(info) = check_test_signature(field, "anonymous function") else {
                        return true;
                    };
                    check_func_body(&fl.body, &info, ge_go124, &options, &mut pending);
                }
                _ => {}
            }
            true
        });
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
                message: String::new(),
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
        name: "usetesting",
        doc: "Reports uses of functions with replacement inside the testing package.",
        url: "https://github.com/ldez/usetesting",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
