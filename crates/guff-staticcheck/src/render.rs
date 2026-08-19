//! Render Go expressions as source-like strings for diagnostic suggestions.

use guff::ast::{CallExpr, Expr, IndexExpr, SelectorExpr, StarExpr, TypeAssertExpr};
use guff::format;
use guff::printer::PrintNode;
use guff::token::Token;
use guff_analysis::Pass;
use guff_types::TypeId;

/// Render `typ` the way upstream does in messages: qualified by import path,
/// except for types declared in the package under analysis, which appear bare.
///
/// This is `types.TypeString(typ, types.RelativeTo(pass.Pkg))`. Passing a nil
/// qualifier instead prints `example.com/pkg.T` where upstream prints `T`.
pub fn type_string_rel(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let this = pass.pkg().types;
    let qf = |pkg: guff_types::PackageId, parena: &guff_types::arena::PackageArena| -> String {
        if Some(pkg) == this {
            String::new()
        } else {
            parena.get(pkg).path().to_string()
        }
    };
    Some(guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        Some(&qf),
    ))
}

/// `report.Render`: `format.Node(&buf, pass.Fset, x)`.
///
/// [`render_expr`] below is a hand-written approximation that falls back to
/// `"<expr>"` for the expression kinds it does not know. That is tolerable in a
/// message about an expression the check has already recognised; it is wrong in
/// a *comparison*, because every kind it does not know renders identically and
/// so compares equal to every other. SA4000 compares, and gitea declares
///
/// ```text
/// [T func(db.EngineMigration) error | func(context.Context, …) error]
/// ```
///
/// whose two union terms are `*ast.FuncType` — two `"<expr>"` renders, and a
/// false "identical expressions on the left and right side of the '|' operator".
pub fn render_node(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    let mut buf = Vec::new();
    format::node(&mut buf, pass.fset(), PrintNode::Expr(expr)).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

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
        Expr::SliceExpr(s) => {
            let part = |e: &Option<Box<Expr>>| e.as_ref().map(|e| render_expr(e)).unwrap_or_default();
            if s.slice3 {
                format!(
                    "{}[{}:{}:{}]",
                    render_expr(&s.x),
                    part(&s.low),
                    part(&s.high),
                    part(&s.max)
                )
            } else {
                format!("{}[{}:{}]", render_expr(&s.x), part(&s.low), part(&s.high))
            }
        }
        // Type expressions. Without these a conversion renders as `<expr>(x)`
        // — S1003 said `bytes.Contains(b, <expr>("x"))` where upstream says
        // `[]byte("x")`.
        Expr::ArrayType(a) => match &a.len {
            Some(len) => format!("[{}]{}", render_expr(len), render_expr(&a.elt)),
            None => format!("[]{}", render_expr(&a.elt)),
        },
        Expr::MapType(m) => format!("map[{}]{}", render_expr(&m.key), render_expr(&m.value)),
        Expr::ChanType(c) => {
            let value = render_expr(&c.value);
            if c.dir == guff::ast::ChanDir::SEND {
                format!("chan<- {value}")
            } else if c.dir == guff::ast::ChanDir::RECV {
                format!("<-chan {value}")
            } else {
                format!("chan {value}")
            }
        }
        Expr::Ellipsis(e) => match &e.elt {
            Some(elt) => format!("...{}", render_expr(elt)),
            None => "...".to_string(),
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
