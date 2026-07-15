//! Shared helpers for revive rules.

use guff::ast::{
    BasicLit, BinaryExpr, CallExpr, Expr, Ident, IndexExpr, SelectorExpr, StarExpr, UnaryExpr,
};
use guff::token::Token;
use guff_analysis::Pass;
use guff_types::arena::TypeData;
use guff_types::basic::{BasicKind, IS_INTEGER};
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

pub fn is_ident(expr: &Expr, name: &str) -> bool {
    matches!(unparen(expr), Expr::Ident(Ident { name: n, .. }) if n == name)
}

pub fn is_blank_ident(expr: &Expr) -> bool {
    matches!(unparen(expr), Expr::Ident(id) if is_blank(id))
}

pub fn is_pkg_dot_type(expr: &Expr, pkg: &str, name: &str) -> bool {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(expr) else {
        return false;
    };
    matches!(unparen(x), Expr::Ident(Ident { name: pkg_name, .. }) if pkg_name == pkg)
        && sel.name == name
}

pub fn is_test_package(pkg_name: &str) -> bool {
    pkg_name.ends_with("_test")
}

pub fn is_importable_package(pkg_name: &str) -> bool {
    pkg_name != "main" && !is_test_package(pkg_name)
}

pub fn first_comment_line(doc: Option<&guff::ast::CommentGroup>) -> String {
    let Some(doc) = doc else {
        return String::new();
    };
    for line in doc.text().lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("Deprecated: ") {
            break;
        }
        return line.to_string();
    }
    String::new()
}

pub fn has_prefix_insensitive(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len()
        && s.chars()
            .zip(prefix.chars())
            .all(|(a, b)| a.eq_ignore_ascii_case(&b))
}

pub fn type_string(pass: &Pass<'_>, typ: TypeId) -> String {
    pass.pkg()
        .type_artifacts
        .as_ref()
        .map(|a| {
            guff_types::typestring::type_string(
                &a.types,
                &a.objects,
                &a.packages,
                typ,
                None,
            )
        })
        .unwrap_or_else(|| "<type>".into())
}

pub fn is_error_ident_type(expr: &Expr) -> bool {
    is_ident(expr, "error")
}

pub fn is_interface_type_expr(expr: &Expr) -> bool {
    matches!(unparen(expr), Expr::InterfaceType(_))
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
        (Expr::IndexExpr(IndexExpr { x: xa, index: ia, .. }), Expr::IndexExpr(IndexExpr { x: xb, index: ib, .. })) => {
            expr_equal(xa, xb) && expr_equal(ia, ib)
        }
        _ => false,
    }
}

pub fn is_named_type(pass: &Pass<'_>, typ: TypeId, pkg: &str, name: &str) -> bool {
    type_string(pass, typ) == format!("{pkg}.{name}")
}

pub fn is_string_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    matches!(
        artifacts.types.get(typ.underlying(&artifacts.types)),
        TypeData::Basic(b) if b.kind() == BasicKind::String
    )
}

pub fn is_integer_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let TypeData::Basic(b) = artifacts.types.get(typ.underlying(&artifacts.types)) else {
        return false;
    };
    if !b.info().contains(IS_INTEGER) {
        return false;
    }
    !matches!(
        b.kind(),
        BasicKind::Uint8 | BasicKind::Int32 | BasicKind::UntypedRune
    )
}

pub fn basic_lit_string_value(lit: &BasicLit) -> Option<&str> {
    if lit.kind != Some(Token::STRING) {
        return None;
    }
    let raw = lit.value.as_str();
    if raw.len() < 2 {
        return None;
    }
    Some(&raw[1..raw.len() - 1])
}

pub fn imports_package(pass: &Pass<'_>, import_path: &str) -> bool {
    if pass.pkg().imports.contains_key(import_path) {
        return true;
    }
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::GenDecl(g) = decl else {
                continue;
            };
            if g.tok != Some(Token::IMPORT) {
                continue;
            }
            for spec in &g.specs {
                let guff::ast::Spec::ImportSpec(is) = spec else {
                    continue;
                };
                if is.path.value.trim_matches('"') == import_path {
                    return true;
                }
            }
        }
    }
    false
}

pub fn expr_string(e: &Expr) -> String {
    match e {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", expr_string(&sel.x), sel.sel.name),
        Expr::StarExpr(s) => format!("*{}", expr_string(&s.x)),
        Expr::ArrayType(a) => {
            let len = a
                .len
                .as_ref()
                .map(|e| expr_string(e))
                .unwrap_or_default();
            format!("[{len}]{}", expr_string(&a.elt))
        }
        Expr::InterfaceType(_) => "interface{}".into(),
        _ => "<type>".into(),
    }
}
