//! Port of [`github.com/golangci/go-printf-func-name`](https://github.com/golangci/go-printf-func-name).

use std::sync::OnceLock;

use guff::ast::{Ellipsis, Expr, Field, FuncDecl};
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn is_empty_interface(expr: &Expr) -> bool {
    match expr {
        Expr::InterfaceType(i) => i.methods.list.is_empty(),
        Expr::Ident(id) => id.name == "any",
        _ => false,
    }
}

fn is_any_ellipsis(ell: &Ellipsis) -> bool {
    match &ell.elt {
        Some(elt) => is_empty_interface(elt),
        None => false,
    }
}

fn is_string_type(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(id) if id.name == "string")
}

fn last_name_is_format(field: &Field) -> bool {
    field
        .names
        .last()
        .is_some_and(|n| n.name == "format")
}

fn check_func(func: &FuncDecl, pending: &mut Vec<(u32, String)>) {
    if let Some(res) = &func.ty.results {
        if !res.list.is_empty() {
            return;
        }
    }

    let Some(params) = &func.ty.params else {
        return;
    };
    if params.list.len() < 2 {
        return;
    }

    let format_param = &params.list[params.list.len() - 2];
    let args_param = &params.list[params.list.len() - 1];

    let Some(format_ty) = &format_param.ty else {
        return;
    };
    if !is_string_type(format_ty) {
        return;
    }
    if !last_name_is_format(format_param) {
        return;
    }

    let Some(args_ty) = &args_param.ty else {
        return;
    };
    let Expr::Ellipsis(ell) = args_ty else {
        return;
    };
    if !is_any_ellipsis(ell) {
        return;
    }

    if func.name.name.ends_with('f') {
        return;
    }

    let name = &func.name.name;
    pending.push((
        func.ty.pos().0 as u32,
        format!("printf-like formatting function '{name}' should be named '{name}f'"),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "goprintffuncname requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::FuncDecl(f) = n {
                check_func(f, &mut pending);
            }
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
        name: "goprintffuncname",
        doc: "checks that printf-like functions are named with `f` at the end",
        url: "https://github.com/golangci/go-printf-func-name",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
