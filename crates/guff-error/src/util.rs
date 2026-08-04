//! Shared helpers for error-related analyzers.

use guff::ast::Expr;
use guff_analysis::code;
use guff_analysis::Pass;
use guff_types::api_predicates::api_implements;
use guff_types::arena::ObjectData;
use guff_types::predicates::is_interface;
use guff_types::{new_pointer, TypeId};

pub fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

pub fn universe_error(pass: &Pass<'_>) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    for oid in artifacts.objects.ids() {
        let ObjectData::TypeName(tn) = artifacts.objects.get(oid) else {
            continue;
        };
        if tn.name() != "error" {
            continue;
        }
        if oid.pkg(&artifacts.objects).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

/// Reports whether `typ` implements the predeclared `error` interface.
pub fn implements_error(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    if code::type_with_name(pass, typ, "error") {
        return true;
    }
    let Some(err) = universe_error(pass) else {
        return false;
    };
    // Silent Invalid types (e.g. failed `[]byte("x")` conversions) must not
    // be treated as errors. The `*T` fallback below would otherwise make
    // `*Invalid` look like it implements `error` (errname FPs on []byte vars).
    if !guff_types::predicates::is_valid(&artifacts.types, typ) {
        return false;
    }
    let mut types = artifacts.types.clone();
    if api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        err,
    ) {
        return true;
    }
    // Also try *T — covers Error() with pointer receiver (errname).
    let ptr = new_pointer(&mut types, typ);
    api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        ptr,
        err,
    )
}

/// Upstream err113 `isError`: expression's type is the predeclared `error` interface.
pub fn is_pure_error(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    if !is_interface(&artifacts.types, typ) {
        return false;
    }
    code::type_with_name(pass, typ, "error")
}

pub fn expr_string(e: &Expr) -> String {
    match e {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", expr_string(&sel.x), sel.sel.name),
        Expr::CallExpr(c) => {
            let args: Vec<String> = c.args.iter().map(expr_string).collect();
            format!("{}({})", expr_string(&c.fun), args.join(", "))
        }
        Expr::ParenExpr(p) => format!("({})", expr_string(&p.x)),
        Expr::StarExpr(s) => format!("*{}", expr_string(&s.x)),
        Expr::UnaryExpr(u) => format!("{}{}", u.op, expr_string(&u.x)),
        Expr::BinaryExpr(b) => format!("{} {} {}", expr_string(&b.x), b.op, expr_string(&b.y)),
        Expr::IndexExpr(i) => format!("{}[{}]", expr_string(&i.x), expr_string(&i.index)),
        Expr::BasicLit(l) => l.value.clone(),
        _ => "<expr>".into(),
    }
}

pub fn unparen(e: &Expr) -> &Expr {
    let mut cur = e;
    while let Expr::ParenExpr(p) = cur {
        cur = &p.x;
    }
    cur
}
