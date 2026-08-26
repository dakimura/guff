//! S1034 — use result of type assertion to simplify cases.
//!
//! Port of `honnef.co/go/tools/simple/s1034`.

use std::sync::OnceLock;

use guff::ast::{Expr, Stmt, TypeAssertExpr, TypeSwitchStmt};
use guff::node_mask;
use guff::walk::{preorder, stmt_ref, NodeRef};
use guff_analysis::code::object_of;
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::render::render_node;
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
) -> Option<Vec<TextEdit>> {
    // Each offender is replaced by `offender.X`, which the walk below has
    // already established is a plain identifier — so the replacement text is a
    // name, and nothing has to be rendered.
    let mut offenders: Vec<TextEdit> = Vec::new();
    let mut unrelated = false;
    for stmt in &clause.body {
        preorder(stmt_ref(stmt), |node| {
            if unrelated {
                return false;
            }
            let NodeRef::TypeAssertExpr(ta) = node else {
                return true;
            };
            let TypeAssertExpr { x: ax, ty, rparen, .. } = ta;
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
            offenders.push(TextEdit {
                pos: ax.pos().0 as u32,
                // `TypeAssertExpr` has no `end()` of its own; it ends at the
                // closing paren of `.(T)`.
                end: (rparen.0 + 1) as u32,
                new_text: id.name.clone(),
            });
            true
        });
        if unrelated {
            return None;
        }
    }
    Some(offenders)
}

/// Upstream only matches `switch x.(type)` (ExprStmt), not `switch y := x.(type)`.
///
/// Returns the guard expression alongside the identifier: the fix replaces the
/// guard with `ident := <guard>`, so it needs the node, not just the name.
fn type_switch_guard<'a>(
    stmt: &'a TypeSwitchStmt,
) -> Option<(&'a guff::ast::Ident, &'a Expr)> {
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
        Expr::Ident(id) => Some((id, &es.x)),
        _ => None,
    }
}

/// The diagnostic and, when upstream would offer one, its edits.
///
/// A clause holding an unrelated assertion contributes no offenders *and*
/// blocks the fix for the whole switch: upstream reports it, because the other
/// clauses still have eliminable assertions, but rewriting only some of them
/// would leave the guard bound to a name the untouched ones do not use.
fn check_type_switch(
    pass: &Pass<'_>,
    stmt: &TypeSwitchStmt,
) -> Option<(u32, String, Option<Vec<TextEdit>>)> {
    let (ident, guard) = type_switch_guard(stmt)?;
    let x = object_of(pass, ident)?;
    let mut offenders: Vec<TextEdit> = Vec::new();
    let mut can_fix = true;
    for stmt_item in &stmt.body.list {
        let Stmt::CaseClause(clause) = stmt_item else {
            continue;
        };
        if clause.list.len() != 1 {
            continue;
        }
        let case_typ = type_of_expr(pass, &clause.list[0])?;
        match clause_offenders(pass, clause, x, case_typ) {
            Some(edits) => offenders.extend(edits),
            None => can_fix = false,
        }
    }
    if offenders.is_empty() {
        return None;
    }
    let message = format!(
        "assigning the result of this type assertion to a variable (switch {} := {}.(type)) could eliminate type assertions in switch cases",
        ident.name, ident.name
    );
    // Upstream reports the guard (`i.(type)`), not the `switch` keyword.
    let pos = guard.pos().0 as u32;
    let edits = if can_fix {
        render_node(pass, guard).map(|g| {
            let mut edits = vec![TextEdit {
                pos,
                end: guard.end().0 as u32,
                new_text: format!("{} := {g}", ident.name),
            }];
            edits.extend(offenders);
            edits
        })
    } else {
        None
    };
    Some((pos, message, edits))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1034 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String, Option<Vec<TextEdit>>)> = Vec::new();
    inspect.preorder_typed(node_mask!(TypeSwitchStmt), pass.files(), |node| {
        let NodeRef::TypeSwitchStmt(stmt) = node else {
            return;
        };
        if let Some(found) = check_type_switch(pass, stmt) {
            pending.push(found);
        }
    });

    for (pos, message, edits) in pending {
        let Some(edits) = edits else {
            pass.report_unless_generated(pos, message);
            continue;
        };
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Simplify type switch".into(),
                text_edits: edits,
            }],
            ..Diagnostic::default()
        });
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
