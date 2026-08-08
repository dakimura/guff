//! S1034 — use result of type assertion to simplify cases.
//!
//! Port of `honnef.co/go/tools/simple/s1034`.

use std::sync::OnceLock;

use guff::ast::{Expr, Stmt, TypeAssertExpr, TypeSwitchStmt};
use guff::node_mask;
use guff::walk::{preorder, stmt_ref, NodeRef};
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::TypeId;

fn type_of_expr(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    pass.types_info()
        .and_then(|info| info.types.get(&expr.id()).map(|tv| tv.typ))
}

fn types_equal(pass: &Pass<'_>, a: TypeId, b: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        a,
        None,
    ) == guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        b,
        None,
    )
}

/// Collect type assertions in `clause` that re-assert the switch tag `x` to the
/// case type. Returns `None` when the clause also contains an unrelated type
/// assertion (upstream skips those clauses — often for symmetry with another
/// value asserted to the same type; vault `pkcs7_test.go`).
fn clause_offenders(
    pass: &Pass<'_>,
    clause: &guff::ast::CaseClause,
    x: guff_types::ObjectId,
    case_typ: TypeId,
) -> Option<usize> {
    let mut offenders = 0usize;
    let mut unrelated = false;
    for stmt in &clause.body {
        preorder(stmt_ref(stmt), |node| {
            if unrelated {
                return false;
            }
            let NodeRef::TypeAssertExpr(TypeAssertExpr { x: ax, ty, .. }) = node else {
                return true;
            };
            let Expr::Ident(id) = ax.as_ref() else {
                unrelated = true;
                return false;
            };
            if object_of(pass, id) != Some(x) {
                unrelated = true;
                return false;
            }
            let Some(ty) = ty.as_ref() else {
                unrelated = true;
                return false;
            };
            let Some(assert_typ) = type_of_expr(pass, ty) else {
                return true;
            };
            if !types_equal(pass, case_typ, assert_typ) {
                unrelated = true;
                return false;
            }
            offenders += 1;
            true
        });
        if unrelated {
            return None;
        }
    }
    Some(offenders)
}

/// Upstream only matches `switch x.(type)` (ExprStmt), not `switch y := x.(type)`.
fn type_switch_ident<'a>(stmt: &'a TypeSwitchStmt) -> Option<&'a guff::ast::Ident> {
    if stmt.init.is_some() {
        return None;
    }
    let Stmt::ExprStmt(es) = &*stmt.assign else {
        return None;
    };
    let Expr::TypeAssertExpr(ta) = &es.x else {
        return None;
    };
    match &*ta.x {
        Expr::Ident(id) => Some(id),
        _ => None,
    }
}

fn check_type_switch(pass: &Pass<'_>, stmt: &TypeSwitchStmt) -> Option<String> {
    let ident = type_switch_ident(stmt)?;
    let x = object_of(pass, ident)?;
    let mut offenders = 0usize;
    for stmt_item in &stmt.body.list {
        let Stmt::CaseClause(clause) = stmt_item else {
            continue;
        };
        if clause.list.len() != 1 {
            continue;
        }
        let case_typ = type_of_expr(pass, &clause.list[0])?;
        if let Some(n) = clause_offenders(pass, clause, x, case_typ) {
            offenders += n;
        }
    }
    if offenders > 0 {
        Some(format!(
            "assigning the result of this type assertion to a variable (switch {} := {}.(type)) could eliminate type assertions in switch cases",
            ident.name, ident.name
        ))
    } else {
        None
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1034 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(TypeSwitchStmt), pass.files(), |node| {
        let NodeRef::TypeSwitchStmt(stmt) = node else {
            return;
        };
        if let Some(msg) = check_type_switch(pass, stmt) {
            // Upstream reports the guard (`i.(type)`), not the `switch` keyword.
            pending.push((stmt.assign.pos().0 as u32, msg));
        }
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1034_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1034",
        doc: "use result of type assertion to simplify cases",
        url: "https://staticcheck.dev/docs/checks/#S1034",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1034_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1034_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
