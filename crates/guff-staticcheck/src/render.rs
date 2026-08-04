//! Render Go expressions as source-like strings for diagnostic suggestions.

use guff::ast::{CallExpr, Expr, IndexExpr, SelectorExpr, StarExpr, TypeAssertExpr};
use guff::token::Token;

/// Render an expression for use in diagnostic messages.
pub fn render_expr(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::BasicLit(lit) => lit.value.clone(),
        Expr::ParenExpr(p) => format!("({})", render_expr(&p.x)),
        Expr::UnaryExpr(u) => format!("{}{}", token_str(u.op), render_expr(&u.x)),
        Expr::BinaryExpr(b) => format!(
            "{} {} {}",
            render_expr(&b.x),
            token_str(b.op),
            render_expr(&b.y)
        ),
        Expr::SelectorExpr(SelectorExpr { x, sel, .. }) => {
            format!("{}.{}", render_expr(x), sel.name)
        }
        Expr::CallExpr(CallExpr { fun, args, .. }) => {
            let mut s = render_expr(fun);
            s.push('(');
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&render_expr(arg));
            }
            s.push(')');
            s
        }
        Expr::CompositeLit(c) => {
            let mut s = String::new();
            if let Some(ty) = &c.ty {
                s.push_str(&render_expr(ty));
            }
            s.push('{');
            for (i, elt) in c.elts.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&render_expr(elt));
            }
            s.push('}');
            s
        }
        Expr::KeyValueExpr(kv) => {
            format!("{}: {}", render_expr(&kv.key), render_expr(&kv.value))
        }
        Expr::IndexExpr(IndexExpr { x, index, .. }) => {
            format!("{}[{}]", render_expr(x), render_expr(index))
        }
        Expr::StarExpr(StarExpr { x, .. }) => format!("*{}", render_expr(x)),
        // Without this, `a.(T).M() == b.(U).M()` collapses to identical
        // `"<expr>.M()"` strings and SA4000 false-positives (traefik).
        Expr::TypeAssertExpr(TypeAssertExpr { x, ty, .. }) => match ty {
            Some(t) => format!("{}.({})", render_expr(x), render_expr(t)),
            None => format!("{}.(type)", render_expr(x)),
        },
        _ => "<expr>".to_string(),
    }
}

fn token_str(op: Token) -> &'static str {
    match op {
        Token::NOT => "!",
        Token::EQL => "==",
        Token::NEQ => "!=",
        Token::LSS => "<",
        Token::LEQ => "<=",
        Token::GTR => ">",
        Token::GEQ => ">=",
        Token::LOR => "||",
        Token::LAND => "&&",
        Token::OR => "|",
        Token::AND => "&",
        Token::ADD => "+",
        Token::SUB => "-",
        Token::MUL => "*",
        Token::QUO => "/",
        Token::REM => "%",
        Token::ARROW => "<-",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff::ast::{BinaryExpr, Ident, UnaryExpr};

    #[test]
    fn renders_ident_and_unary() {
        let x = Expr::Ident(Ident::new_ident("x"));
        assert_eq!(render_expr(&x), "x");
        let not_x = Expr::UnaryExpr(UnaryExpr {
            op_pos: Default::default(),
            op: Token::NOT,
            x: Box::new(x),
            id: 0,
        });
        assert_eq!(render_expr(&not_x), "!x");
    }

    #[test]
    fn renders_logical_and_or_distinctly() {
        let lhs = Expr::Ident(Ident::new_ident("a"));
        let rhs = Expr::Ident(Ident::new_ident("b"));
        let land = Expr::BinaryExpr(BinaryExpr {
            x: Box::new(lhs.clone()),
            op_pos: Default::default(),
            op: Token::LAND,
            y: Box::new(rhs.clone()),
            id: 0,
        });
        let lor = Expr::BinaryExpr(BinaryExpr {
            x: Box::new(lhs),
            op_pos: Default::default(),
            op: Token::LOR,
            y: Box::new(rhs),
            id: 0,
        });
        assert_eq!(render_expr(&land), "a && b");
        assert_eq!(render_expr(&lor), "a || b");
        assert_ne!(render_expr(&land), render_expr(&lor));
    }
}
