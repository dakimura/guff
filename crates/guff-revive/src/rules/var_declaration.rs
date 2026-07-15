//! `var-declaration` — drop redundant type or zero-value from var declarations.

use guff::ast::{Expr, GenDecl, Ident, Spec, ValueSpec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_blank, is_ident, is_interface_type_expr, type_of, unparen};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::GenDecl(g) = n {
                if g.tok == Some(Token::VAR) {
                    for spec in &g.specs {
                        if let Spec::ValueSpec(vs) = spec {
                            check_value_spec(pass, vs, &mut failures);
                        }
                    }
                }
            }
            true
        });
    }
    failures
}

fn check_value_spec(pass: &Pass<'_>, vs: &ValueSpec, failures: &mut Vec<Failure>) {
    if vs.names.len() != 1 || vs.ty.is_none() || vs.values.is_empty() {
        return;
    }
    let name = &vs.names[0];
    if is_blank(name) {
        return;
    }
    let ty = vs.ty.as_ref().expect("checked");
    let rhs = &vs.values[0];
    if is_zero_rhs(pass, rhs, ty) {
        failures.push(Failure {
            rule: "var-declaration",
            pos: rhs.pos().0 as u32,
            message: format!(
                "should drop = {} from declaration of var {}; it is the zero value",
                expr_lit(rhs),
                name.name
            ),
        });
        return;
    }
    if is_interface_type_expr(ty) {
        return;
    }
    let Some(lhs_typ) = type_of(pass, ty) else {
        return;
    };
    let Some(rhs_typ) = type_of(pass, rhs) else {
        return;
    };
    if lhs_typ != rhs_typ {
        return;
    }
    failures.push(Failure {
        rule: "var-declaration",
        pos: ty.pos().0 as u32,
        message: format!(
            "should omit type {} from declaration of var {}; it will be inferred from the right-hand side",
            crate::util::expr_string(ty),
            name.name
        ),
    });
}

fn is_zero_rhs(_pass: &Pass<'_>, rhs: &Expr, ty: &Expr) -> bool {
    if is_ident(rhs, "nil") {
        return is_interface_type_expr(ty)
            || matches!(unparen(ty), Expr::Ident(Ident { name, .. }) if name == "any");
    }
    let Expr::BasicLit(lit) = unparen(rhs) else {
        return false;
    };
    match lit.value.as_str() {
        "false" | "\"\"" | "``" | "0" | "0." | "0.0" | "0i" | "'\\x00'" | "'\\000'" => true,
        _ => false,
    }
}

fn expr_lit(expr: &Expr) -> String {
    match unparen(expr) {
        Expr::BasicLit(l) => l.value.clone(),
        Expr::Ident(id) => id.name.clone(),
        _ => "<expr>".into(),
    }
}
