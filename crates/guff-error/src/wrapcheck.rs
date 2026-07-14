//! Port of [`github.com/tomarrell/wrapcheck`](https://github.com/tomarrell/wrapcheck).
//!
//! Default ignore signatures match upstream. Interface/package-glob config is
//! DEFERRED (defaults only).

use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, Ident, ReturnStmt, SelectorExpr};
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::{ObjectId, TypeData};
use guff_types::predicates::is_interface;

use crate::util::{is_pure_error, type_of, unparen};

const DEFAULT_IGNORE_SIGS: &[&str] = &[
    ".Errorf(",
    "errors.New(",
    "errors.Unwrap(",
    "errors.Join(",
    ".Wrap(",
    ".Wrapf(",
    ".WithMessage(",
    ".WithMessagef(",
    ".WithStack(",
];

fn is_error_typ(pass: &Pass<'_>, typ: guff_types::TypeId) -> bool {
    guff_analysis::code::type_with_name(pass, typ, "error")
}

fn call_sig(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    let name = code::call_name(pass, &call.fun)?;
    Some(format!("{name}("))
}

fn ignored_sig(sig: &str) -> bool {
    DEFAULT_IGNORE_SIGS.iter().any(|p| sig.contains(p))
}

fn object_of_ident(pass: &Pass<'_>, id: &Ident) -> Option<ObjectId> {
    let info = pass.types_info()?;
    info.uses
        .get(&id.id)
        .copied()
        .or_else(|| info.defs.get(&id.id).copied().flatten())
}

fn sel_func_obj(pass: &Pass<'_>, sel: &SelectorExpr) -> Option<ObjectId> {
    let info = pass.types_info()?;
    info.uses.get(&sel.sel.id).copied()
}

fn is_from_other_pkg(pass: &Pass<'_>, sel: &SelectorExpr) -> bool {
    let Some(obj) = sel_func_obj(pass, sel) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(pkg) = obj.pkg(&artifacts.objects) else {
        return false;
    };
    let path = artifacts.packages.get(pkg).path();
    path != pass.pkg().pkg_path && !path.is_empty()
}

fn is_iface_method(pass: &Pass<'_>, sel: &SelectorExpr) -> bool {
    let Some(typ) = type_of(pass, &sel.x) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    is_interface(&artifacts.types, typ)
        && sel.sel.name.chars().next().is_some_and(|c| c.is_uppercase())
}

fn report_unwrapped(pass: &Pass<'_>, call: &CallExpr, pos: u32, pending: &mut Vec<(u32, String)>) {
    if let Expr::Ident(_) = unparen(&call.fun) {
        // Package-internal Ident call — ignored unless ReportInternalErrors (default false).
        return;
    }
    let Expr::SelectorExpr(sel) = unparen(&call.fun) else {
        return;
    };
    let Some(sig) = call_sig(pass, call) else {
        return;
    };
    if ignored_sig(&sig) {
        return;
    }
    if is_iface_method(pass, sel) && is_from_other_pkg(pass, sel) {
        pending.push((
            pos,
            format!("error returned from interface method should be wrapped: sig: {sig}"),
        ));
        return;
    }
    if is_from_other_pkg(pass, sel) {
        pending.push((
            pos,
            format!("error returned from external package is unwrapped: sig: {sig}"),
        ));
    }
}

fn call_returns_error(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(typ) = type_of(pass, &Expr::CallExpr(call.clone())) else {
        // Fall back: inspect results via call type
        let info = match pass.types_info() {
            Some(i) => i,
            None => return false,
        };
        return info
            .types
            .get(&call.id)
            .is_some_and(|tav| is_error_typ(pass, tav.typ) || is_tuple_with_error(pass, tav.typ));
    };
    is_error_typ(pass, typ) || is_tuple_with_error(pass, typ)
}

fn is_tuple_with_error(pass: &Pass<'_>, typ: guff_types::TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    match artifacts.types.get(typ) {
        TypeData::Tuple(t) => (0..t.len()).any(|i| {
            t.at(i)
                .typ(&artifacts.objects)
                .is_some_and(|rt| is_error_typ(pass, rt))
        }),
        _ => false,
    }
}

fn prev_err_assign<'a>(
    pass: &Pass<'_>,
    file: &'a guff::ast::File,
    return_ident: &Ident,
) -> Option<&'a AssignStmt> {
    let ret_obj = object_of_ident(pass, return_ident)?;
    let ret_pos = return_ident.name_pos.0;
    let mut most_recent: Option<&AssignStmt> = None;
    walk::preorder(NodeRef::File(file), |n| {
        let NodeRef::AssignStmt(ass) = n else {
            return true;
        };
        if ass.tok_pos.0 as i64 > ret_pos {
            return true;
        }
        for lhs in &ass.lhs {
            let Expr::Ident(id) = unparen(lhs) else {
                continue;
            };
            if object_of_ident(pass, id) == Some(ret_obj) {
                most_recent = Some(ass);
            }
        }
        true
    });
    most_recent
}

fn check_return(
    pass: &Pass<'_>,
    file: &guff::ast::File,
    ret: &ReturnStmt,
    stack: &[NodeRef<'_>],
    pending: &mut Vec<(u32, String)>,
) {
    // Skip returns inside FuncLit.
    for n in stack.iter().rev() {
        match n {
            NodeRef::FuncLit(_) => return,
            NodeRef::FuncDecl(_) => break,
            _ => {}
        }
    }

    for expr in &ret.results {
        if let Expr::CallExpr(call) = unparen(expr) {
            if call_returns_error(pass, call) {
                report_unwrapped(pass, call, call.lparen.0 as u32, pending);
            }
            continue;
        }
        if !is_pure_error(pass, expr) {
            continue;
        }
        let Expr::Ident(ident) = unparen(expr) else {
            continue;
        };
        let Some(ass) = prev_err_assign(pass, file, ident) else {
            continue;
        };
        if ass.rhs.len() != 1 {
            continue;
        }
        let Expr::CallExpr(call) = unparen(&ass.rhs[0]) else {
            continue;
        };
        report_unwrapped(pass, call, ident.name_pos.0 as u32, pending);
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "wrapcheck requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    for file in pass.files() {
        let mut stack = Vec::new();
        walk::preorder_stack(NodeRef::File(file), &mut stack, |n, stack| {
            if let NodeRef::ReturnStmt(ret) = n {
                check_return(pass, file, ret, stack, &mut pending);
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
        name: "wrapcheck",
        doc: "Checks that errors returned from external packages are wrapped",
        url: "https://github.com/tomarrell/wrapcheck",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
