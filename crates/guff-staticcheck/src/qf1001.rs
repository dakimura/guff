//! QF1001 — apply De Morgan's law.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1001`.
//!
use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::token::Token;
use guff::walk::{NodeMask, NodeRef, expr_ref, preorder};
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

struct RenderedNegation {
    text: String,
    top_op: Option<Token>,
}

/// Render De Morgan negation of `expr` as source text.
///
/// When `recursive` is false, parenthesized subexpressions are left as `!(...)`
/// rather than being rewritten inside. When `simplify_parentheses` is true,
/// parentheses that are redundant after the recursive rewrite are omitted.
fn negate_de_morgan(expr: &Expr, recursive: bool, simplify_parentheses: bool) -> String {
    negate_de_morgan_inner(expr, recursive, simplify_parentheses, None).text
}

fn negate_de_morgan_inner(
    expr: &Expr,
    recursive: bool,
    simplify_parentheses: bool,
    parent_op: Option<Token>,
) -> RenderedNegation {
    match expr {
        Expr::BinaryExpr(b) => {
            let (op, rendered_op, nx, ny) = match b.op {
                Token::EQL => (Token::NEQ, "!=", render_expr(&b.x), render_expr(&b.y)),
                Token::NEQ => (Token::EQL, "==", render_expr(&b.x), render_expr(&b.y)),
                Token::LSS => (Token::GEQ, ">=", render_expr(&b.x), render_expr(&b.y)),
                Token::GTR => (Token::LEQ, "<=", render_expr(&b.x), render_expr(&b.y)),
                Token::LEQ => (Token::GTR, ">", render_expr(&b.x), render_expr(&b.y)),
                Token::GEQ => (Token::LSS, "<", render_expr(&b.x), render_expr(&b.y)),
                Token::LAND => {
                    let x = negate_de_morgan_inner(
                        &b.x,
                        recursive,
                        simplify_parentheses,
                        Some(Token::LOR),
                    );
                    let y = negate_de_morgan_inner(
                        &b.y,
                        recursive,
                        simplify_parentheses,
                        Some(Token::LOR),
                    );
                    (Token::LOR, "||", x.text, y.text)
                }
                Token::LOR => {
                    let x = negate_de_morgan_inner(
                        &b.x,
                        recursive,
                        simplify_parentheses,
                        Some(Token::LAND),
                    );
                    let y = negate_de_morgan_inner(
                        &b.y,
                        recursive,
                        simplify_parentheses,
                        Some(Token::LAND),
                    );
                    (Token::LAND, "&&", x.text, y.text)
                }
                _ => {
                    return RenderedNegation {
                        text: format!("!{}", wrap_if_needed(expr)),
                        top_op: None,
                    }
                }
            };
            RenderedNegation {
                text: format!("{nx} {rendered_op} {ny}"),
                top_op: Some(op),
            }
        }
        Expr::ParenExpr(p) => {
            if recursive {
                let inner = negate_de_morgan_inner(&p.x, true, simplify_parentheses, parent_op);
                if simplify_parentheses && !needs_parentheses(inner.top_op, parent_op) {
                    inner
                } else {
                    RenderedNegation {
                        text: format!("({})", inner.text),
                        top_op: None,
                    }
                }
            } else {
                RenderedNegation {
                    text: format!("!{}", render_expr(expr)),
                    top_op: None,
                }
            }
        }
        Expr::UnaryExpr(u) if u.op == Token::NOT => RenderedNegation {
            text: render_expr(&u.x),
            top_op: None,
        },
        Expr::UnaryExpr(_) => RenderedNegation {
            text: format!("!{}", wrap_if_needed(expr)),
            top_op: None,
        },
        _ => RenderedNegation {
            text: format!("!{}", wrap_if_needed(expr)),
            top_op: None,
        },
    }
}

fn needs_parentheses(child_op: Option<Token>, parent_op: Option<Token>) -> bool {
    let Some(child_op) = child_op else {
        return false;
    };
    let Some(parent_op) = parent_op else {
        return false;
    };
    precedence(child_op) < precedence(parent_op)
}

fn precedence(op: Token) -> u8 {
    match op {
        Token::LOR => 1,
        Token::LAND => 2,
        Token::EQL | Token::NEQ | Token::LSS | Token::LEQ | Token::GTR | Token::GEQ => 3,
        _ => 4,
    }
}

fn wrap_if_needed(expr: &Expr) -> String {
    match expr {
        Expr::Ident(_)
        | Expr::BasicLit(_)
        | Expr::SelectorExpr(_)
        | Expr::CallExpr(_)
        | Expr::IndexExpr(_)
        | Expr::ParenExpr(_) => render_expr(expr),
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
    const WANTED: NodeMask = node_mask!(
        BinaryExpr,
        ForStmt,
        IfStmt,
        SwitchStmt,
    );
    inspect.preorder_typed(WANTED, pass.files(), |node| {
        if needed {
            return;
        }
        match node {
            NodeRef::IfStmt(i) if contains_pos(&i.cond, unary_pos) => needed = true,
            NodeRef::ForStmt(f) if f.cond.as_ref().is_some_and(|c| contains_pos(c, unary_pos)) => {
                needed = true;
            }
            NodeRef::SwitchStmt(s)
                if s.tag.as_ref().is_some_and(|t| contains_pos(t, unary_pos)) =>
            {
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

    let mut pending: Vec<(u32, u32, Vec<(String, String)>)> = Vec::new();
    inspect.preorder_typed(node_mask!(UnaryExpr), pass.files(), |node| {
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
        let bn = negate_de_morgan(inner, false, false);
        let bnr = negate_de_morgan(inner, true, false);
        let bns = negate_de_morgan(inner, false, true);
        let bnrs = negate_de_morgan(inner, true, true);
        let wrap = needs_outer_parens(pass, u.op_pos.0 as u32);
        let bn = if wrap { format!("({bn})") } else { bn };
        let bnr = if wrap { format!("({bnr})") } else { bnr };
        let bns = if wrap { format!("({bns})") } else { bns };
        let bnrs = if wrap { format!("({bnrs})") } else { bnrs };
        let mut fixes = vec![("Apply De Morgan's law".to_string(), bn.clone())];
        for (message, text) in [
            ("Apply De Morgan's law recursively", bnr),
            ("Apply De Morgan's law and simplify parentheses", bns),
            (
                "Apply De Morgan's law recursively and simplify parentheses",
                bnrs,
            ),
        ] {
            if fixes.iter().all(|(_, existing)| existing != &text) {
                fixes.push((message.to_string(), text));
            }
        }
        pending.push((
            u.op_pos.0 as u32,
            // UnaryExpr ends at the end of its operand.
            u.x.end().0 as u32,
            fixes,
        ));
    });

    for (pos, end, fixes) in pending {
        let fixes = fixes
            .into_iter()
            .map(|(message, new_text)| SuggestedFix {
                message,
                text_edits: vec![TextEdit { pos, end, new_text }],
            })
            .collect();
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
    use guff::parser_interface::parse_expr;
    use guff_analysis::validate;

    #[test]
    fn qf1001_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn recursive_fix_can_simplify_redundant_parentheses() {
        let expr = parse_expr("!(a && (b || c))").unwrap();
        let Expr::UnaryExpr(u) = expr else {
            panic!("expected unary expression");
        };
        let inner = unparen(&u.x);
        assert_eq!(negate_de_morgan(inner, true, false), "!a || (!b && !c)");
        assert_eq!(negate_de_morgan(inner, true, true), "!a || !b && !c");
    }

    #[test]
    fn simplified_parentheses_preserve_precedence() {
        let expr = parse_expr("!(a || (b && c))").unwrap();
        let Expr::UnaryExpr(u) = expr else {
            panic!("expected unary expression");
        };
        let inner = unparen(&u.x);
        assert_eq!(negate_de_morgan(inner, true, true), "!a && (!b || !c)");
    }
}
