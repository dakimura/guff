//! `get-return` — warn on getter-like functions that return nothing.

use guff::ast::{Decl, Expr, FuncType};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::unparen;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            if !is_getter(&f.name.name) {
                continue;
            }
            if has_results(&f.ty) {
                continue;
            }
            if is_http_handler(&f.ty) {
                continue;
            }
            failures.push(Failure {
                rule: "get-return",
                pos: f.name.name_pos.0 as u32,
                message: format!(
                    "function '{}' seems to be a getter but it does not return any result",
                    f.name.name
                ),
            });
        }
    }
    failures
}

fn is_getter(name: &str) -> bool {
    const PREFIX: &str = "Get";
    if !name.starts_with(PREFIX) {
        return false;
    }
    if name.len() == PREFIX.len() {
        return false;
    }
    name.as_bytes()[PREFIX.len()].is_ascii_uppercase()
}

fn has_results(ty: &FuncType) -> bool {
    ty.results.as_ref().is_some_and(|r| !r.list.is_empty())
}

fn is_http_handler(ty: &FuncType) -> bool {
    let Some(params) = &ty.params else {
        return false;
    };
    let types = param_type_names(params);
    types.len() >= 2 && types[0] == "http.ResponseWriter" && types[1] == "*http.Request"
}

fn param_type_names(params: &guff::ast::FieldList) -> Vec<String> {
    let mut out = Vec::new();
    for field in &params.list {
        let Some(ty) = &field.ty else {
            continue;
        };
        let n = field.names.len().max(1);
        let name = type_name(ty);
        for _ in 0..n {
            out.push(name.clone());
        }
    }
    out
}

fn type_name(expr: &Expr) -> String {
    match unparen(expr) {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", type_name(&sel.x), sel.sel.name),
        Expr::StarExpr(star) => format!("*{}", type_name(&star.x)),
        _ => String::new(),
    }
}
