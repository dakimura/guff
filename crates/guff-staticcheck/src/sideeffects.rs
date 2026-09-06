//! `honnef.co/go/tools/analysis/code.MayHaveSideEffects`, with a nil purity
//! result.
//!
//! Six checks call it that way upstream — QF1002, QF1003, QF1005, S1009,
//! SA4014 and SA5002 — and each of them had grown its own copy here, four
//! identical and two cruder still. All six were missing the same thing: the
//! `*ast.UnaryExpr` arm recurses into the operand **and then** answers
//! `expr.Op == token.ARROW || expr.Op == token.AND`, so taking an address and
//! receiving from a channel are side effects in themselves. SA4014's copy was
//! `matches!(expr, Expr::CallExpr(_))`, which does not even see a call nested
//! inside a comparison.
//!
//! **SA4018 is deliberately not here.** It is the one upstream site that
//! passes a real `purity.Result`, where a call to a function proven pure is
//! *not* a side effect; sharing this would silently change what it accepts.
//! (`code.go`'s `*ast.CallExpr` arm returns true immediately only when the
//! purity result is nil.)
//!
//! Upstream panics on an expression kind it does not handle, so it has no
//! "everything else is pure" default; the `_ => false` below stands for its
//! `Ident` / `BasicLit` / `FuncLit` / type-expression arms, which all return
//! false.

use guff::ast::Expr;
use guff::token::Token;

/// Whether evaluating `expr` may have a side effect.
pub(crate) fn may_have_side_effects(expr: &Expr) -> bool {
    match expr {
        Expr::BadExpr(_) => true,
        Expr::CallExpr(_) => true,
        Expr::UnaryExpr(u) => {
            may_have_side_effects(&u.x) || u.op == Token::ARROW || u.op == Token::AND
        }
        Expr::BinaryExpr(b) => may_have_side_effects(&b.x) || may_have_side_effects(&b.y),
        Expr::IndexExpr(i) => may_have_side_effects(&i.x) || may_have_side_effects(&i.index),
        Expr::IndexListExpr(i) => {
            may_have_side_effects(&i.x) || i.indices.iter().any(may_have_side_effects)
        }
        Expr::SelectorExpr(s) => may_have_side_effects(&s.x),
        Expr::StarExpr(s) => may_have_side_effects(&s.x),
        Expr::ParenExpr(p) => may_have_side_effects(&p.x),
        Expr::TypeAssertExpr(t) => may_have_side_effects(&t.x),
        Expr::KeyValueExpr(kv) => may_have_side_effects(&kv.key) || may_have_side_effects(&kv.value),
        Expr::CompositeLit(c) => {
            c.ty.as_deref().is_some_and(may_have_side_effects)
                || c.elts.iter().any(may_have_side_effects)
        }
        Expr::Ellipsis(e) => e.elt.as_deref().is_some_and(may_have_side_effects),
        Expr::SliceExpr(s) => {
            may_have_side_effects(&s.x)
                || s.low.as_ref().is_some_and(|e| may_have_side_effects(e))
                || s.high.as_ref().is_some_and(|e| may_have_side_effects(e))
                || s.max.as_ref().is_some_and(|e| may_have_side_effects(e))
        }
        _ => false,
    }
}
