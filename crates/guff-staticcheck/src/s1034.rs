//! S1034 — use result of type assertion to simplify cases.
//!
//! Port of `honnef.co/go/tools/simple/s1034`.

use std::sync::OnceLock;

use guff::ast::{Expr, Stmt, TypeAssertExpr, TypeSwitchStmt};
use guff::walk::NodeRef;
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::TypeId;

fn render_type(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let a = pass.pkg().type_artifacts.as_ref()?;
    Some(guff_types::typestring::type_string(
        &a.types,
        &a.objects,
        &a.packages,
        typ,
        None,
    ))
}

fn count_redundant_assertions(
    pass: &Pass<'_>,
    stmt: &guff::ast::Stmt,
    x: guff_types::ObjectId,
    case_typ: TypeId,
) -> usize {
    let mut count = 0;
    walk_stmt(pass, stmt, x, case_typ, &mut count);
    count
}

fn walk_stmt(
    pass: &Pass<'_>,
    stmt: &guff::ast::Stmt,
    x: guff_types::ObjectId,
    case_typ: TypeId,
    count: &mut usize,
) {
    use guff::ast::Stmt;
    match stmt {
        Stmt::AssignStmt(a) => {
            for e in a.lhs.iter().chain(a.rhs.iter()) {
                walk_expr(pass, e, x, case_typ, count);
            }
        }
        Stmt::ExprStmt(e) => walk_expr(pass, &e.x, x, case_typ, count),
        Stmt::BlockStmt(b) => {
            for s in &b.list {
                walk_stmt(pass, s, x, case_typ, count);
            }
        }
        _ => {}
    }
}

fn walk_expr(
    pass: &Pass<'_>,
    expr: &Expr,
    x: guff_types::ObjectId,
    case_typ: TypeId,
    count: &mut usize,
) {
    if let Expr::TypeAssertExpr(TypeAssertExpr { x: ax, ty, .. }) = expr {
        if let (Expr::Ident(id), Some(ty)) = (&**ax, ty.as_ref()) {
            if object_of(pass, id) == Some(x) {
                let assert_typ = pass
                    .types_info()
                    .and_then(|info| info.types.get(&ty.id()).map(|tv| tv.typ));
                if let Some(at) = assert_typ {
                    if render_type(pass, at) == render_type(pass, case_typ) {
                        *count += 1;
                    }
                }
            }
        }
    }
    match expr {
        Expr::BinaryExpr(e) => {
            walk_expr(pass, &e.x, x, case_typ, count);
            walk_expr(pass, &e.y, x, case_typ, count);
        }
        Expr::CallExpr(e) => {
            for a in &e.args {
                walk_expr(pass, a, x, case_typ, count);
            }
        }
        _ => {}
    }
}

fn type_switch_ident<'a>(stmt: &'a TypeSwitchStmt) -> Option<&'a guff::ast::Ident> {
    if let Stmt::AssignStmt(assign) = &*stmt.assign {
        let Expr::TypeAssertExpr(ta) = assign.rhs.first()? else {
            return None;
        };
        match &*ta.x {
            Expr::Ident(id) => Some(id),
            _ => None,
        }
    } else if let Stmt::ExprStmt(es) = &*stmt.assign {
        let Expr::TypeAssertExpr(ta) = &es.x else {
            return None;
        };
        match &*ta.x {
            Expr::Ident(id) => Some(id),
            _ => None,
        }
    } else {
        None
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
        let case_typ = pass
            .types_info()
            .and_then(|info| info.types.get(&clause.list[0].id()).map(|tv| tv.typ))?;
        for s in &clause.body {
            offenders += count_redundant_assertions(pass, s, x, case_typ);
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
    inspect.preorder(pass.files(), |node| {
        let NodeRef::TypeSwitchStmt(stmt) = node else {
            return;
        };
        if let Some(msg) = check_type_switch(pass, stmt) {
            pending.push((match_pos(node), msg));
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
