//! Port of [`mvdan.cc/unparam`](https://github.com/mvdan/unparam)
//! (golangci-lint wrapper in `pkg/golinters/unparam`).
//!
//! Reports unused function parameters. This is an AST-based approximation:
//! a parameter is unused when its name does not appear as an identifier in the
//! function body (excluding `_ = param` intentional keeps).
//!
//! Upstream also checks unused / constant results and uses SSA for interface
//! satisfaction, forwarded calls, and call-graph precision.
//!
//! DEFERRED: SSA-based analysis (`buildir`), unused/constant results,
//! interface-satisfaction skips, generated-file skips, recursive-only uses.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{Decl, Expr, FuncDecl, FuncLit, Stmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use crate::options::UnparamOptions;

fn is_blank_param(name: &str) -> bool {
    name.is_empty() || name.starts_with('_')
}

fn recv_type_string(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::StarExpr(s) => format!("*{}", recv_type_string(&s.x)),
        _ => "?".to_string(),
    }
}

fn func_display_name(fd: &FuncDecl) -> String {
    if let Some(recv) = &fd.recv {
        if let Some(field) = recv.list.first() {
            if let Some(ty) = &field.ty {
                return format!("({}).{}", recv_type_string(ty), fd.name.name);
            }
        }
    }
    fd.name.name.clone()
}

fn is_stub_body(body: &guff::ast::BlockStmt) -> bool {
    if body.list.is_empty() {
        return true;
    }
    if body.list.len() == 1 {
        return matches!(
            &body.list[0],
            Stmt::ReturnStmt(ret) if ret.results.is_empty(),
        ) || matches!(
            &body.list[0],
            Stmt::ExprStmt(e) if matches!(&e.x, Expr::CallExpr(call) if is_panic_or_log_call(call))
        );
    }
    false
}

fn is_panic_or_log_call(call: &guff::ast::CallExpr) -> bool {
    let Expr::Ident(id) = &*call.fun else {
        return false;
    };
    matches!(
        id.name.as_str(),
        "panic" | "print" | "println" | "log" | "logf" | "logln"
    )
}

fn intentional_keep(body: &guff::ast::BlockStmt, param: &str) -> bool {
    let mut kept = false;
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(NodeRef::AssignStmt(asgn)) = n else {
            return true;
        };
        if asgn.tok != Some(Token::ASSIGN) || asgn.lhs.len() != 1 || asgn.rhs.len() != 1 {
            return true;
        }
        let (Expr::Ident(blank), Expr::Ident(rhs)) = (&asgn.lhs[0], &asgn.rhs[0]) else {
            return true;
        };
        if blank.name == "_" && rhs.name == param {
            kept = true;
        }
        true
    });
    kept
}

fn collect_used_idents(body: &guff::ast::BlockStmt) -> HashSet<String> {
    let mut used = HashSet::new();
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        if let Some(NodeRef::Ident(id)) = n {
            used.insert(id.name.clone());
        }
        true
    });
    used
}

fn check_params(
    func_name: &str,
    params: &[guff::ast::Field],
    body: &guff::ast::BlockStmt,
    pending: &mut Vec<(u32, String)>,
) {
    if is_stub_body(body) {
        return;
    }
    let used = collect_used_idents(body);
    for field in params {
        for name in &field.names {
            let pname = &name.name;
            if is_blank_param(pname) {
                continue;
            }
            if used.contains(pname) || intentional_keep(body, pname) {
                continue;
            }
            pending.push((
                name.name_pos.0 as u32,
                format!("{func_name} - {pname} is unused"),
            ));
        }
    }
}

fn should_check_exported(pass: &Pass<'_>, fd: &FuncDecl, check_exported: bool) -> bool {
    if check_exported || pass.pkg().name == "main" {
        return true;
    }
    if fd.name.name.contains('$') {
        return true;
    }
    !fd.name.is_exported()
}

fn check_func_decl(
    pass: &Pass<'_>,
    fd: &FuncDecl,
    check_exported: bool,
    pending: &mut Vec<(u32, String)>,
) {
    if fd.name.name == "init" {
        return;
    }
    let Some(body) = &fd.body else {
        return;
    };
    if !should_check_exported(pass, fd, check_exported) {
        return;
    }
    let Some(params) = &fd.ty.params else {
        return;
    };
    let func_name = func_display_name(fd);
    check_params(&func_name, &params.list, body, pending);
}

fn check_func_lit(lit: &FuncLit, pending: &mut Vec<(u32, String)>) {
    let Some(params) = &lit.ty.params else {
        return;
    };
    check_params("<func literal>", &params.list, &lit.body, pending);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "unparam requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<UnparamOptions>("unparam")
        .copied()
        .unwrap_or_default();

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            check_func_decl(pass, fd, opts.check_exported, &mut pending);
        }
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::FuncLit(lit)) = n else {
                return true;
            };
            check_func_lit(lit, &mut pending);
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
        name: "unparam",
        doc: "Reports unused function parameters",
        url: "https://github.com/mvdan/unparam",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
