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

/// errorlint's `exprToString` — a hand-rolled walker, **not** `go/printer`.
///
/// Its `BinaryExpr` arm is `X + " " + Op + " " + Y`, so it always puts blanks
/// around an operator where `go/printer` would drop them by precedence. That is
/// the upstream behaviour and must not be "fixed" by pointing it at
/// [`guff_analysis::code::node_text`]; durationcheck and err113 used to borrow
/// this function and *were* wrong to, because both of those render with
/// `go/printer` upstream.
///
/// Reaches only errorlint's suggested-fix text, never a message — which is why
/// the golden tier cannot see it.
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
        Expr::TypeAssertExpr(t) => format!(
            "{}.({})",
            expr_string(&t.x),
            t.ty.as_deref().map(expr_string).unwrap_or_default()
        ),
        // Upstream's default arm is this literal string, comment syntax and all.
        _ => "/* complex expression */".into(),
    }
}

pub fn unparen(e: &Expr) -> &Expr {
    let mut cur = e;
    while let Expr::ParenExpr(p) = cur {
        cur = &p.x;
    }
    cur
}

#[cfg(test)]
mod expr_string_tests {
    use super::expr_string;
    use guff::parser_interface::parse_expr;

    /// `exprToString` reaches only errorlint's suggested-fix text, so no golden
    /// case can see it. Pin it against upstream's arms directly instead.
    ///
    /// Source: go-errorlint@v1.8.0 `errorlint/lint.go:358`.
    #[test]
    fn matches_upstream_arms() {
        for (src, want) in [
            ("err", "err"),
            ("io.EOF", "io.EOF"),
            ("*p", "*p"),
            ("-n", "-n"),
            // Always blanks, even where go/printer would drop them by
            // precedence — upstream concatenates with literal spaces.
            ("a/2 + b", "a / 2 + b"),
            ("wrap(ctx, e)", "wrap(ctx, e)"),
            ("(err)", "(err)"),
            ("errs[0]", "errs[0]"),
            (r#""lit""#, r#""lit""#),
            ("err.(*MyErr)", "err.(*MyErr)"),
            // Outside the handled arms upstream emits this literal string.
            ("func() error { return nil }", "/* complex expression */"),
            ("[]error{err}", "/* complex expression */"),
        ] {
            let expr = parse_expr(src).unwrap_or_else(|e| panic!("parse {src}: {e:?}"));
            assert_eq!(expr_string(&expr), want, "rendering {src}");
        }
    }
}
