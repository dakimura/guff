//! `ifaceassert` — check for impossible interface type assertions.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, CaseClause, Expr, ExprStmt, Stmt, TypeAssertExpr, TypeSwitchStmt};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::check_lookup::missing_method;
use guff_types::predicates::is_interface;

use crate::expreq::unparen;
use crate::govet_util::expr_type;

fn iface_conflict(pass: &Pass<'_>, v: guff_types::TypeId, t: guff_types::TypeId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if !is_interface(&artifacts.types, v) || !is_interface(&artifacts.types, t) {
        return None;
    }
    let mut types = artifacts.types.clone();
    let mm = missing_method(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        v,
        t,
        false,
    )?;
    if !mm.wrong_type {
        return None;
    }
    let vname = guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        v,
        None,
    );
    let tname = guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        t,
        None,
    );
    let mname = mm.method.name(&artifacts.objects);
    Some(format!(
        "impossible type assertion: no type can implement both {vname} and {tname} (conflicting types for {mname} method)"
    ))
}

fn type_from_expr(pass: &Pass<'_>, e: &Expr) -> Option<guff_types::TypeId> {
  match unparen(e) {
    Expr::Ident(_) | Expr::SelectorExpr(_) | Expr::StarExpr(_) | Expr::IndexExpr(_) => expr_type(pass, e),
    _ => expr_type(pass, e),
  }
}

fn check_assert(pass: &Pass<'_>, assert: &TypeAssertExpr, targets: &[&Expr]) -> Vec<(u32, String)> {
    let Some(v) = expr_type(pass, &assert.x) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for target in targets {
        let Some(t) = type_from_expr(pass, target) else {
            continue;
        };
        if let Some(msg) = iface_conflict(pass, v, t) {
            out.push((target.pos().0 as u32, msg));
        }
    }
    out
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ifaceassert requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(TypeAssertExpr, TypeSwitchStmt), pass.files(), |n| {
        match n {
            NodeRef::TypeAssertExpr(ta) => {
                let Some(t) = ta.ty.as_deref() else {
                    return;
                };
                pending.extend(check_assert(pass, ta, &[t]));
            }
            NodeRef::TypeSwitchStmt(TypeSwitchStmt { assign, body, .. }) => {
                let assert = match &**assign {
                    Stmt::ExprStmt(ExprStmt { x, .. }) => match unparen(x) {
                        Expr::TypeAssertExpr(ta) => ta,
                        _ => return,
                    },
                    Stmt::AssignStmt(AssignStmt { rhs, .. }) => match rhs.first().map(|e| unparen(e)) {
                        Some(Expr::TypeAssertExpr(ta)) => ta,
                        _ => return,
                    },
                    _ => return,
                };
                let targets: Vec<&Expr> = body
                    .list
                    .iter()
                    .filter_map(|s| {
                        let Stmt::CaseClause(CaseClause { list, .. }) = s else {
                            return None;
                        };
                        Some(list.iter().collect::<Vec<_>>())
                    })
                    .flatten()
                    .collect();
                pending.extend(check_assert(pass, assert, &targets));
            }
            _ => {}
        }
    });
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "ifaceassert",
        doc: "detect impossible interface type assertions",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/ifaceassert",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
