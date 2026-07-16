//! QF1001 — apply De Morgan's law.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1001`.
//!
//! DEFERRED: `SimplifyParentheses` variants of the SuggestedFix (upstream offers
//! up to 4 fixes; we offer non-recursive and recursive De Morgan only).

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::token::Token;
use guff::walk::{expr_ref, preorder, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::alias::unalias_readonly;
use guff_types::predicates::is_float;

use crate::render::render_expr;

fn unparen(expr: &Expr) -> &Expr {
    match expr {
        Expr::ParenExpr(p) => unparen(&p.x),
        _ => expr,
    }
}

fn has_floats(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut found = false;
    preorder(expr_ref(expr), |node| {
        if found {
            return false;
        }
        // Check typed expression nodes via their AST id when available.
        let id = match node {
            NodeRef::Ident(e) => e.id(),
            NodeRef::BasicLit(e) => e.id,
            NodeRef::ParenExpr(e) => e.id,
            NodeRef::SelectorExpr(e) => e.id,
            NodeRef::IndexExpr(e) => e.id,
            NodeRef::CallExpr(e) => e.id,
            NodeRef::StarExpr(e) => e.id,
            NodeRef::UnaryExpr(e) => e.id,
            NodeRef::BinaryExpr(e) => e.id,
            NodeRef::SliceExpr(e) => e.id,
            NodeRef::TypeAssertExpr(e) => e.id,
            NodeRef::CompositeLit(e) => e.id,
            NodeRef::IndexListExpr(e) => e.id,
            NodeRef::KeyValueExpr(e) => e.id,
            _ => return true,
        };
        if let Some(tav) = info.types.get(&id) {
            let typ = unalias_readonly(&artifacts.types, tav.typ);
            if is_float(&artifacts.types, typ) {
                found = true;
                return false;
            }
        }
        true
    });
    found
}

/// Render De Morgan negation of `expr` as source text.
///
/// When `recursive` is false, parenthesized subexpressions are left as `!(...)`
/// rather than being rewritten inside.
fn negate_de_morgan(expr: &Expr, recursive: bool) -> String {
    match expr {
        Expr::BinaryExpr(b) => {
            let (op, nx, ny) = match b.op {
                Token::EQL => ("!=", render_expr(&b.x), render_expr(&b.y)),
                Token::NEQ => ("==", render_expr(&b.x), render_expr(&b.y)),
                Token::LSS => (">=", render_expr(&b.x), render_expr(&b.y)),
                Token::GTR => ("<=", render_expr(&b.x), render_expr(&b.y)),
                Token::LEQ => (">", render_expr(&b.x), render_expr(&b.y)),
                Token::GEQ => ("<", render_expr(&b.x), render_expr(&b.y)),
                Token::LAND => (
                    "||",
                    negate_de_morgan(&b.x, recursive),
                    negate_de_morgan(&b.y, recursive),
                ),
                Token::LOR => (
                    "&&",
                    negate_de_morgan(&b.x, recursive),
                    negate_de_morgan(&b.y, recursive),
                ),
                _ => return format!("!{}", wrap_if_needed(expr)),
            };
            format!("{nx} {op} {ny}")
        }
        Expr::ParenExpr(p) => {
            if recursive {
                format!("({})", negate_de_morgan(&p.x, true))
            } else {
                format!("!{}", render_expr(expr))
            }
        }
        Expr::UnaryExpr(u) if u.op == Token::NOT => render_expr(&u.x),
        Expr::UnaryExpr(_) => format!("!{}", wrap_if_needed(expr)),
        _ => format!("!{}", wrap_if_needed(expr)),
    }
}

fn wrap_if_needed(expr: &Expr) -> String {
    match expr {
        Expr::Ident(_) | Expr::BasicLit(_) | Expr::SelectorExpr(_) | Expr::CallExpr(_)
        | Expr::IndexExpr(_) | Expr::ParenExpr(_) => render_expr(expr),
        _ => format!("({})", render_expr(expr)),
    }
}

fn needs_outer_parens(pass: &Pass<'_>, unary_pos: u32) -> bool {
    // Heuristic: if the `!` sits in an if/for/switch condition or as a binary
    // operand, keep parentheses so precedence stays correct after rewrite.
    // Walk files looking for BinaryExpr/IfStmt/ForStmt/SwitchStmt that contain
    // this unary as a direct child (via position containment of cond/operands).
    let mut needed = false;
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .cloned();
    let Some(inspect) = inspect else {
        return false;
    };
    inspect.preorder(pass.files(), |node| {
        if needed {
            return;
        }
        match node {
            NodeRef::IfStmt(i) if contains_pos(&i.cond, unary_pos) => needed = true,
            NodeRef::ForStmt(f) if f.cond.as_ref().is_some_and(|c| contains_pos(c, unary_pos)) => {
                needed = true;
            }
            NodeRef::SwitchStmt(s) if s.tag.as_ref().is_some_and(|t| contains_pos(t, unary_pos)) => {
                needed = true;
            }
            NodeRef::BinaryExpr(b)
                if contains_pos(&b.x, unary_pos) || contains_pos(&b.y, unary_pos) =>
            {
                // Only force parens when this unary is a *direct* operand of another binary.
                if is_unary_not_at(&b.x, unary_pos) || is_unary_not_at(&b.y, unary_pos) {
                    needed = true;
                }
            }
            _ => {}
        }
    });
    needed
}

fn contains_pos(expr: &Expr, pos: u32) -> bool {
    expr.pos().0 as u32 <= pos && pos < expr.end().0 as u32
}

fn is_unary_not_at(expr: &Expr, pos: u32) -> bool {
    matches!(expr, Expr::UnaryExpr(u) if u.op == Token::NOT && u.op_pos.0 as u32 == pos)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1001 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String, String, bool)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::UnaryExpr(u) = node else {
            return;
        };
        if u.op != Token::NOT {
            return;
        }
        // Match `!BinaryExpr` or `!(BinaryExpr)` (unparen).
        let inner = unparen(&u.x);
        let Expr::BinaryExpr(_) = inner else {
            return;
        };
        if has_floats(pass, inner) {
            return;
        }
        let bn = negate_de_morgan(inner, false);
        let bnr = negate_de_morgan(inner, true);
        let wrap = needs_outer_parens(pass, u.op_pos.0 as u32);
        let bn = if wrap {
            format!("({bn})")
        } else {
            bn
        };
        let bnr = if wrap {
            format!("({bnr})")
        } else {
            bnr
        };
        pending.push((
            u.op_pos.0 as u32,
            // UnaryExpr ends at the end of its operand.
            u.x.end().0 as u32,
            bn,
            bnr,
            true,
        ));
    });

    for (pos, end, bn, bnr, _) in pending {
        let mut fixes = vec![SuggestedFix {
            message: "Apply De Morgan's law".into(),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text: bn.clone(),
            }],
        }];
        if bn != bnr {
            fixes.push(SuggestedFix {
                message: "Apply De Morgan's law recursively".into(),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: bnr,
                }],
            });
        }
        pass.report(Diagnostic {
            pos,
            end,
            message: "could apply De Morgan's law".into(),
            suggested_fixes: fixes,
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn qf1001_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1001",
        doc: "apply De Morgan's law",
        url: "https://staticcheck.dev/docs/checks/#QF1001",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1001_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1001_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
