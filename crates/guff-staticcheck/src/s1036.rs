//! S1036 — unnecessary guard around map access.
//!
//! Port of `honnef.co/go/tools/simple/s1036`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, CompositeLit, Expr, Ident, IfStmt, IncDecStmt, IndexExpr, Stmt};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{expr_to_int, is_call_to, object_of};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn same_expr(pass: &Pass<'_>, a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(ia), Expr::Ident(ib)) => {
            object_of(pass, ia) == object_of(pass, ib) || ia.name == ib.name
        }
        (Expr::IndexExpr(ia), Expr::IndexExpr(ib)) => {
            same_expr(pass, &ia.x, &ib.x) && same_expr(pass, &ia.index, &ib.index)
        }
        _ => a.id() == b.id(),
    }
}

fn map_index_init(init: &Stmt) -> Option<&IndexExpr> {
    let Stmt::AssignStmt(AssignStmt { lhs, rhs, tok, .. }) = init else {
        return None;
    };
    if tok != &Some(Token::DEFINE) || lhs.len() != 2 || rhs.len() != 1 {
        return None;
    };
    let Expr::Ident(blank) = &lhs[0] else {
        return None;
    };
    if blank.name != "_" {
        return None;
    };
    let Expr::IndexExpr(ix) = &rhs[0] else {
        return None;
    };
    Some(ix)
}

fn same_map_index(pass: &Pass<'_>, lhs: &Expr, ix: &IndexExpr) -> bool {
    let Expr::IndexExpr(lhs_ix) = lhs else {
        return false;
    };
    same_expr(pass, &lhs_ix.x, &ix.x) && same_expr(pass, &lhs_ix.index, &ix.index)
}

fn else_assign_stmt<'a>(else_: &'a Stmt) -> Option<&'a AssignStmt> {
    match else_ {
        Stmt::AssignStmt(a) => Some(a),
        Stmt::BlockStmt(b) if b.list.len() == 1 => match &b.list[0] {
            Stmt::AssignStmt(a) => Some(a),
            _ => None,
        },
        _ => None,
    }
}

fn check_append_guard(pass: &Pass<'_>, ifs: &IfStmt, ix: &IndexExpr) -> bool {
    let Some(else_) = ifs.else_.as_deref() else {
        return false;
    };
    if ifs.body.list.len() != 1 {
        return false;
    }
    let Stmt::AssignStmt(then) = &ifs.body.list[0] else {
        return false;
    };
    let Some(else_assign) = else_assign_stmt(else_) else {
        return false;
    };
    if !matches!(then.tok, Some(Token::ASSIGN)) || then.lhs.len() != 1 || then.rhs.len() != 1 {
        return false;
    }
    if !same_map_index(pass, &then.lhs[0], ix) {
        return false;
    }
    let Expr::CallExpr(call) = &then.rhs[0] else {
        return false;
    };
    if !is_call_to(pass, call, "append") || call.args.len() != 2 || !same_expr(pass, &call.args[0], &then.lhs[0])
    {
        return false;
    }
    matches!(else_assign.tok, Some(Token::ASSIGN))
        && else_assign.lhs.len() == 1
        && same_expr(pass, &else_assign.lhs[0], &then.lhs[0])
        && matches!(else_assign.rhs.first(), Some(Expr::CompositeLit(CompositeLit { elts, .. })) if elts.len() == 1 && same_expr(pass, &elts[0], &call.args[1]))
}

fn check_add_guard(pass: &Pass<'_>, ifs: &IfStmt, ix: &IndexExpr) -> bool {
    let Some(else_) = ifs.else_.as_deref() else {
        return false;
    };
    let Stmt::AssignStmt(then) = &ifs.body.list[0] else {
        return false;
    };
    let Some(else_assign) = else_assign_stmt(else_) else {
        return false;
    };
    matches!(then.tok, Some(Token::AddAssign))
        && then.lhs.len() == 1
        && then.rhs.len() == 1
        && same_map_index(pass, &then.lhs[0], ix)
        && matches!(else_assign.tok, Some(Token::ASSIGN))
        && same_expr(pass, &else_assign.lhs[0], &then.lhs[0])
        && same_expr(pass, &else_assign.rhs[0], &then.rhs[0])
}

fn check_inc_guard(pass: &Pass<'_>, ifs: &IfStmt, ix: &IndexExpr) -> bool {
    let Some(else_) = ifs.else_.as_deref() else {
        return false;
    };
    let Stmt::IncDecStmt(IncDecStmt { x, tok, .. }) = &ifs.body.list[0] else {
        return false;
    };
    let Some(else_assign) = else_assign_stmt(else_) else {
        return false;
    };
    *tok == Token::INC
        && same_map_index(pass, x, ix)
        && matches!(else_assign.tok, Some(Token::ASSIGN))
        && else_assign.lhs.len() == 1
        && else_assign.lhs[0].id() == x.id()
        && else_assign.rhs.len() == 1
        && expr_to_int(pass, &else_assign.rhs[0]).is_some_and(|n| n == 1)
}

fn check_if(pass: &Pass<'_>, ifs: &IfStmt) -> bool {
    let Some(ix) = ifs.init.as_deref().and_then(map_index_init) else {
        return false;
    };
    let Expr::Ident(ok) = &ifs.cond else {
        return false;
    };
    if ok.name == "_" || ifs.body.list.len() != 1 {
        return false;
    }
    check_append_guard(pass, ifs, ix) || check_add_guard(pass, ifs, ix) || check_inc_guard(pass, ifs, ix)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1036 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::IfStmt(ifs) = node else {
            return;
        };
        if check_if(pass, ifs) {
            pending.push((match_pos(node), "unnecessary guard around map access".into()));
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1036_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1036",
        doc: "unnecessary guard around map access",
        url: "https://staticcheck.dev/docs/checks/#S1036",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1036_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1036_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
