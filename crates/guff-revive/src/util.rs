//! Shared helpers for revive rules.

use guff::ast::{BasicLit, Expr, Ident, SelectorExpr};
use guff::token::Token;
use guff_analysis::Pass;
use guff_types::TypeId;

pub fn unparen<'a>(expr: &'a Expr) -> &'a Expr {
    match expr {
        Expr::ParenExpr(p) => unparen(&p.x),
        other => other,
    }
}

pub fn is_blank(ident: &Ident) -> bool {
    ident.name == "_"
}

pub fn is_pkg_dot_name(fun: &Expr, pkg: &str, name: &str) -> bool {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(fun) else {
        return false;
    };
    matches!(unparen(x), Expr::Ident(Ident { name: pkg_name, .. }) if pkg_name == pkg)
        && sel.name == name
}

pub fn basic_lit_string(lit: &BasicLit) -> Option<&str> {
    if lit.kind != Some(Token::STRING) {
        return None;
    }
    let raw = lit.value.as_str();
    if raw.len() < 2 {
        return None;
    }
    Some(&raw[1..raw.len() - 1])
}

pub fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

pub fn is_duration_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let s = guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    );
    s == "time.Duration" || s == "*time.Duration"
}

pub fn receiver_type_key(recv_ty: &Expr) -> String {
    match unparen(recv_ty) {
        Expr::Ident(id) => id.name.clone(),
        Expr::StarExpr(star) => format!("*{}", receiver_type_key(&star.x)),
        Expr::SelectorExpr(sel) => {
            let pkg = match unparen(&sel.x) {
                Expr::Ident(id) => id.name.clone(),
                other => format!("{other:?}"),
            };
            format!("{pkg}.{}", sel.sel.name)
        }
        other => format!("{other:?}"),
    }
}
