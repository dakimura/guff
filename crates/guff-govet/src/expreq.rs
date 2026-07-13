//! Structural expression helpers for govet checks.

use guff::ast::{
    BasicLit, BinaryExpr, CallExpr, CompositeLit, Expr, Ident, IndexExpr, ParenExpr, SelectorExpr,
    StarExpr, UnaryExpr,
};
use guff::token::Token;

pub fn unparen<'a>(e: &'a Expr) -> &'a Expr {
    let mut cur = e;
    while let Expr::ParenExpr(ParenExpr { x, .. }) = cur {
        cur = x;
    }
    cur
}

pub fn expr_equal(a: &Expr, b: &Expr) -> bool {
    match (unparen(a), unparen(b)) {
        (Expr::Ident(Ident { name: na, .. }), Expr::Ident(Ident { name: nb, .. })) => na == nb,
        (
            Expr::BasicLit(BasicLit { value: va, kind: ka, .. }),
            Expr::BasicLit(BasicLit { value: vb, kind: kb, .. }),
        ) => va == vb && ka == kb,
        (
            Expr::SelectorExpr(SelectorExpr { x: xa, sel: sa, .. }),
            Expr::SelectorExpr(SelectorExpr { x: xb, sel: sb, .. }),
        ) => sa.name == sb.name && expr_equal(xa, xb),
        (Expr::StarExpr(StarExpr { x: xa, .. }), Expr::StarExpr(StarExpr { x: xb, .. })) => {
            expr_equal(xa, xb)
        }
        (
            Expr::UnaryExpr(UnaryExpr { op: oa, x: xa, .. }),
            Expr::UnaryExpr(UnaryExpr { op: ob, x: xb, .. }),
        ) => oa == ob && expr_equal(xa, xb),
        (
            Expr::BinaryExpr(BinaryExpr { op: oa, x: xa, y: ya, .. }),
            Expr::BinaryExpr(BinaryExpr { op: ob, x: xb, y: yb, .. }),
        ) => oa == ob && expr_equal(xa, xb) && expr_equal(ya, yb),
        (
            Expr::CallExpr(CallExpr { fun: fa, args: aa, .. }),
            Expr::CallExpr(CallExpr { fun: fb, args: ab, .. }),
        ) => aa.len() == ab.len() && expr_equal(fa, fb) && aa.iter().zip(ab).all(|(x, y)| expr_equal(x, y)),
        (
            Expr::IndexExpr(IndexExpr { x: xa, index: ia, .. }),
            Expr::IndexExpr(IndexExpr { x: xb, index: ib, .. }),
        ) => expr_equal(xa, xb) && expr_equal(ia, ib),
        (Expr::CompositeLit(ca), Expr::CompositeLit(cb)) => {
            let same_ty = match (&ca.ty, &cb.ty) {
                (Some(a), Some(b)) => expr_equal(a, b),
                (None, None) => true,
                _ => false,
            };
            same_ty && ca.elts.len() == cb.elts.len()
        }
        _ => false,
    }
}

pub fn token_is_shift(op: Token) -> bool {
    matches!(op, Token::SHL | Token::SHR)
}

pub fn token_is_shift_assign(op: Option<Token>) -> bool {
    matches!(op, Some(Token::ShlAssign) | Some(Token::ShrAssign))
}
