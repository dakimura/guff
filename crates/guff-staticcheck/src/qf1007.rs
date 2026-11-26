//! QF1007 — merge conditional assignment into variable declaration.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1007`.
//!
//! Recognizes:
//! ```ignore
//! x := false
//! if cond {
//!     x = true
//! }
//! ```
//! and suggests `x := cond` (or `x := !cond` when the initial value is `true`).

use std::sync::OnceLock;

use guff::ast::{BlockStmt, Expr, FuncDecl, FuncLit, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::ObjectId;

use crate::render::render_expr;

/// `(stmt_pos, stmt_end, rhs_pos, rhs_end, if_pos, if_end, replacement)`: the
/// declaration statement upstream reports, plus the two ranges the fix edits.
type PendingMerge = (u32, u32, u32, u32, u32, u32, String);

fn bool_lit(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Ident(id) if id.name == "true" => Some(true),
        Expr::Ident(id) if id.name == "false" => Some(false),
        _ => None,
    }
}

fn define_bool(
    pass: &Pass<'_>,
    stmt: &Stmt,
) -> Option<(ObjectId, u32, u32, bool)> {
    let Stmt::AssignStmt(assign) = stmt else {
        return None;
    };
    if assign.tok != Some(Token::DEFINE) || assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return None;
    }
    let Expr::Ident(lhs) = &assign.lhs[0] else {
        return None;
    };
    let obj = object_of(pass, lhs)?;
    let init = bool_lit(&assign.rhs[0])?;
    Some((
        obj,
        assign.rhs[0].pos().0 as u32,
        assign.rhs[0].end().0 as u32,
        init,
    ))
}

fn if_assign_bool<'a>(
    pass: &Pass<'_>,
    stmt: &'a Stmt,
) -> Option<(ObjectId, &'a Expr, bool, u32, u32)> {
    let Stmt::IfStmt(if_) = stmt else {
        return None;
    };
    if if_.init.is_some() || if_.else_.is_some() {
        return None;
    }
    if if_.body.list.len() != 1 {
        return None;
    }
    let Stmt::AssignStmt(assign) = &if_.body.list[0] else {
        return None;
    };
    if assign.tok != Some(Token::ASSIGN) || assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return None;
    }
    let Expr::Ident(lhs) = &assign.lhs[0] else {
        return None;
    };
    let obj = object_of(pass, lhs)?;
    let set = bool_lit(&assign.rhs[0])?;
    Some((
        obj,
        &if_.cond,
        set,
        stmt.pos().0 as u32,
        stmt.end().0 as u32,
    ))
}

fn check_body(pass: &Pass<'_>, body: &BlockStmt, pending: &mut Vec<PendingMerge>) {
    let stmts = &body.list;
    if stmts.len() < 2 {
        return;
    }
    for i in 0..stmts.len() - 1 {
        let Some((obj1, rhs_pos, rhs_end, init)) = define_bool(pass, &stmts[i]) else {
            continue;
        };
        let Some((obj2, cond, set, if_pos, if_end)) = if_assign_bool(pass, &stmts[i + 1]) else {
            continue;
        };
        if obj1 != obj2 || init == set {
            continue;
        }
        let replacement = if init {
            format!("!{}", render_factor_for_not(cond))
        } else {
            render_expr(cond)
        };
        // Upstream reports the declaration statement; the edits still target
        // the RHS and the `if` that is folded into it.
        pending.push((
            stmts[i].pos().0 as u32,
            stmts[i].end().0 as u32,
            rhs_pos,
            rhs_end,
            if_pos,
            if_end,
            replacement,
        ));
    }
}

fn render_factor_for_not(expr: &Expr) -> String {
    match expr {
        Expr::Ident(_) | Expr::SelectorExpr(_) | Expr::ParenExpr(_) | Expr::CallExpr(_) => {
            render_expr(expr)
        }
        _ => format!("({})", render_expr(expr)),
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1007 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<PendingMerge> = Vec::new();
    inspect.preorder_typed(node_mask!(FuncDecl, FuncLit), pass.files(), |node| {
        match node {
            NodeRef::FuncDecl(FuncDecl { body: Some(body), .. }) => {
                check_body(pass, body, &mut pending);
            }
            NodeRef::FuncLit(FuncLit { body, .. }) => {
                check_body(pass, body, &mut pending);
            }
            _ => {}
        }
    });

    for (stmt_pos, stmt_end, rhs_pos, rhs_end, if_pos, if_end, replacement) in pending {
        pass.report(Diagnostic {
            pos: stmt_pos,
            end: stmt_end,
            message: "could merge conditional assignment into variable declaration".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Merge conditional assignment into variable declaration".into(),
                text_edits: vec![
                    TextEdit {
                        pos: rhs_pos,
                        end: rhs_end,
                        new_text: replacement,
                    },
                    TextEdit {
                        pos: if_pos,
                        end: if_end,
                        new_text: String::new(),
                    },
                ],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn qf1007_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1007",
        doc: "merge conditional assignment into variable declaration",
        url: "https://staticcheck.dev/docs/checks/#QF1007",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1007_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1007_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
