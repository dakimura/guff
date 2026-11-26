//! Port of [`github.com/gostaticanalysis/forcetypeassert`](https://github.com/gostaticanalysis/forcetypeassert).

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, TypeAssertExpr, ValueSpec};
use guff::walk::{self, expr_ref, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn unparen(e: &Expr) -> &Expr {
    let mut cur = e;
    while let Expr::ParenExpr(p) = cur {
        cur = &p.x;
    }
    cur
}

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<guff_types::TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn is_any(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    code::type_with_name(pass, typ, "any") || code::type_with_name(pass, typ, "interface{}")
}

fn find_type_assertion(exprs: &[Expr]) -> Option<&TypeAssertExpr> {
    for expr in exprs {
        let mut found = None;
        // Upstream is `ast.Inspect`, so both `return false`s prune a subtree and
        // the walk continues over the siblings. That has two consequences guff
        // was missing by stopping the walk outright: an assertion *after* a
        // closure is still found, and when an expression holds several, the
        // assignment is overwritten — so the answer is the last one, not the
        // first.
        walk::preorder_prune(expr_ref(expr), |n| {
            match n {
                NodeRef::FuncLit(_) => return false,
                NodeRef::TypeAssertExpr(ta) => {
                    found = Some(ta);
                    return false;
                }
                _ => {}
            }
            true
        });
        if found.is_some() {
            return found;
        }
    }
    None
}

fn is_call_expr(expr: &Expr) -> bool {
    matches!(unparen(expr), Expr::CallExpr(_))
}

fn check_assign(pass: &Pass<'_>, n: &AssignStmt) -> Option<(u32, String)> {
    let tae = find_type_assertion(&n.rhs)?;
    // `pass.Reportf(n.Pos(), …)` — an `*ast.AssignStmt`'s `Pos()` is its first
    // left-hand expression, not the `:=`. guff pointed at the token, which is
    // the same *line* and so invisible to every gate whose key stops there.
    let pos = n.lhs.first().map(|e| e.pos().0 as u32).unwrap_or(n.tok_pos.0 as u32);
    if n.rhs.len() == 1 && is_call_expr(&n.rhs[0]) {
        return Some((pos, "right hand must be only type assertion".into()));
    }
    if n.rhs.len() > 1 {
        return Some((pos, "right hand must be only type assertion".into()));
    }
    if n.lhs.len() != 2 {
        if tae.ty.as_ref().is_some_and(|t| !is_any(pass, t)) {
            return Some((pos, "type assertion must be checked".into()));
        }
    }
    None
}

fn check_value_spec(pass: &Pass<'_>, n: &ValueSpec) -> Option<(u32, String)> {
    let tae = find_type_assertion(&n.values)?;
    let pos = n.names.first().map(|id| id.name_pos.0 as u32).unwrap_or(0);
    if n.values.len() == 1 && is_call_expr(&n.values[0]) {
        return Some((pos, "right hand must be only type assertion".into()));
    }
    if n.values.len() > 1 {
        return Some((pos, "right hand must be only type assertion".into()));
    }
    if n.names.len() != 2 {
        if tae.ty.as_ref().is_some_and(|t| !is_any(pass, t)) {
            return Some((pos, "type assertion must be checked".into()));
        }
    }
    None
}

fn check_type_assert(pass: &Pass<'_>, n: &TypeAssertExpr) -> Option<(u32, String)> {
    let ty = n.ty.as_deref()?;
    if is_any(pass, ty) {
        return None;
    }
    // Same rule: an `*ast.TypeAssertExpr`'s `Pos()` is its operand's, not the
    // `(` of the assertion.
    Some((n.x.pos().0 as u32, "type assertion must be checked".into()))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "forcetypeassert requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::AssignStmt(s) => {
                    if find_type_assertion(&s.rhs).is_some() {
                        if let Some(diag) = check_assign(pass, s) {
                            pending.push(diag);
                        }
                        return false;
                    }
                    true
                }
                NodeRef::ValueSpec(vs) => {
                    if find_type_assertion(&vs.values).is_some() {
                        if let Some(diag) = check_value_spec(pass, vs) {
                            pending.push(diag);
                        }
                        return false;
                    }
                    true
                }
                NodeRef::TypeAssertExpr(ta) => {
                    if let Some(diag) = check_type_assert(pass, ta) {
                        pending.push(diag);
                    }
                    false
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
        name: "forcetypeassert",
        doc: "finds type assertions which force the conversion without checking ok",
        url: "https://github.com/gostaticanalysis/forcetypeassert",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
