//! QF1006 — lift `if`+`break` into loop condition.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1006`.
//!
//! Recognizes:
//! ```ignore
//! for {
//!     if cond {
//!         break
//!     }
//!     ...
//! }
//! ```
//! and suggests `for !cond { ... }`.

use std::sync::OnceLock;

use guff::ast::{Expr, ForStmt, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::render::render_expr;

/// Negate an expression for a loop condition (non-recursive De Morgan for `&&`/`||`).
fn negate_expr(expr: &Expr) -> String {
    match expr {
        Expr::UnaryExpr(u) if u.op == Token::NOT => render_expr(&u.x),
        Expr::BinaryExpr(b) => match b.op {
            Token::EQL => format!("{} != {}", render_expr(&b.x), render_expr(&b.y)),
            Token::NEQ => format!("{} == {}", render_expr(&b.x), render_expr(&b.y)),
            Token::LSS => format!("{} >= {}", render_expr(&b.x), render_expr(&b.y)),
            Token::GTR => format!("{} <= {}", render_expr(&b.x), render_expr(&b.y)),
            Token::LEQ => format!("{} > {}", render_expr(&b.x), render_expr(&b.y)),
            Token::GEQ => format!("{} < {}", render_expr(&b.x), render_expr(&b.y)),
            Token::LAND => format!("{} || {}", negate_expr(&b.x), negate_expr(&b.y)),
            Token::LOR => format!("{} && {}", negate_expr(&b.x), negate_expr(&b.y)),
            _ => format!("!({})", render_expr(expr)),
        },
        Expr::ParenExpr(_) => format!("!{}", render_expr(expr)),
        Expr::Ident(_) | Expr::SelectorExpr(_) | Expr::CallExpr(_) | Expr::IndexExpr(_) => {
            format!("!{}", render_expr(expr))
        }
        _ => format!("!({})", render_expr(expr)),
    }
}

fn is_break_only(body: &guff::ast::BlockStmt) -> bool {
    if body.list.len() != 1 {
        return false;
    }
    matches!(
        &body.list[0],
        Stmt::BranchStmt(b) if b.tok == Token::BREAK && b.label.is_none()
    )
}

fn check_for(for_stmt: &ForStmt, pending: &mut Vec<(u32, u32, u32, String)>) {
    if for_stmt.init.is_some() || for_stmt.cond.is_some() || for_stmt.post.is_some() {
        return;
    }
    if for_stmt.body.list.is_empty() {
        return;
    }
    let if_stmt = &for_stmt.body.list[0];
    let Stmt::IfStmt(if_) = if_stmt else {
        return;
    };
    if if_.init.is_some() || if_.else_.is_some() {
        return;
    }
    if !is_break_only(&if_.body) {
        return;
    }
    let negated = negate_expr(&if_.cond);
    // Insert after `for` (3 bytes).
    let insert_pos = for_stmt.for_.0 as u32 + 3;
    pending.push((
        if_stmt.pos().0 as u32,
        if_stmt.end().0 as u32,
        insert_pos,
        format!(" {negated}"),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1006 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(ForStmt), pass.files(), |node| {
        let NodeRef::ForStmt(for_stmt) = node else {
            return;
        };
        check_for(for_stmt, &mut pending);
    });

    for (if_pos, if_end, insert_pos, cond_text) in pending {
        pass.report(Diagnostic {
            pos: if_pos,
            end: if_end,
            message: "could lift into loop condition".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Lift into loop condition".into(),
                text_edits: vec![
                    TextEdit {
                        pos: insert_pos,
                        end: insert_pos,
                        new_text: cond_text,
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

fn qf1006_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1006",
        doc: "lift if+break into loop condition",
        url: "https://staticcheck.dev/docs/checks/#QF1006",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1006_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1006_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
