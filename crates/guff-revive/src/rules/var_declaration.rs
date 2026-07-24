//! `var-declaration` — drop redundant type or zero-value from var declarations.

use guff::ast::{Expr, Ident, Spec, ValueSpec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;
use guff_types::predicates::is_untyped;

use crate::failure::Failure;
use crate::util::{is_blank, is_ident, is_interface_type_expr, type_of, unparen};

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn new(pass: &'a Pass<'a>) -> Self {
        Self {
            pass,
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
            
                    if let NodeRef::GenDecl(g) = n {
                        if g.tok == Some(Token::VAR) {
                            for spec in &g.specs {
                                if let Spec::ValueSpec(vs) = spec {
                                    check_value_spec(self.pass, vs, &mut self.failures);
                                }
                            }
                        }
                    }
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new(pass);
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
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
        failures.push(Failure::new(
            "var-declaration",
            rhs.pos().0 as u32,
            format!(
                "should drop = {} from declaration of var {}; it is the zero value",
                expr_lit(rhs),
                name.name
            ),
        ));
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
    // Cross-pkg untyped consts can appear typed in Types due to assignment
    // context, while go/types Identical(typed, untyped) is false. If the RHS
    // names an untyped const (object type) but the Types entry is typed, skip.
    if rhs_names_untyped_const(pass, rhs) && !types_entry_is_untyped(pass, rhs) {
        return;
    }
    // Upstream re-evals the RHS outside assignment context. Untyped consts
    // (e.g. `math.MaxInt64`, `5`) take the LHS type in Types, so Identical
    // succeeds — only warn when the LHS is the const's default type (`int`).
    if let Some(def) = untyped_const_default_name(pass, rhs) {
        if !is_ident(ty, def) {
            return;
        }
    }
    failures.push(Failure::new(
        "var-declaration",
        ty.pos().0 as u32,
        format!(
            "should omit type {} from declaration of var {}; it will be inferred from the right-hand side",
            crate::util::expr_string(ty),
            name.name
        ),
    ));
}

/// Approximate revive's `File.IsUntypedConst`: detect RHS expressions that are
/// untyped constants (literals, named untyped consts, or ops over them) and
/// return their default type name (`"int"`, `"float64"`, …).
fn untyped_const_default_name(pass: &Pass<'_>, expr: &Expr) -> Option<&'static str> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;

    // Prefer Types entry when still recorded as untyped. If Types has a typed
    // entry, do not fall through to AST (assignment-context typing).
    if let Some(info) = pass.types_info() {
        if let Some(tv) = info.types.get(&expr.id()) {
            if let Some(name) = untyped_basic_default(&artifacts.types, tv.typ) {
                return Some(name);
            }
            return None;
        }
    }

    match unparen(expr) {
        Expr::BasicLit(lit) => match lit.kind {
            Some(Token::INT) => Some("int"),
            Some(Token::FLOAT) => Some("float64"),
            Some(Token::IMAG) => Some("complex128"),
            Some(Token::CHAR) => Some("rune"),
            Some(Token::STRING) => Some("string"),
            _ => None,
        },
        Expr::Ident(id) if id.name == "true" || id.name == "false" => Some("bool"),
        Expr::Ident(id) => const_ident_default(pass, id.id()),
        Expr::SelectorExpr(sel) => const_ident_default(pass, sel.sel.id()),
        Expr::UnaryExpr(u) if matches!(u.op, Token::ADD | Token::SUB | Token::XOR) => {
            untyped_const_default_name(pass, &u.x)
        }
        Expr::BinaryExpr(b)
            if matches!(
                b.op,
                Token::ADD
                    | Token::SUB
                    | Token::MUL
                    | Token::QUO
                    | Token::REM
                    | Token::AND
                    | Token::OR
                    | Token::XOR
                    | Token::SHL
                    | Token::SHR
            ) =>
        {
            let l = untyped_const_default_name(pass, &b.x)?;
            let r = untyped_const_default_name(pass, &b.y)?;
            Some(max_default_name(l, r))
        }
        Expr::ParenExpr(p) => untyped_const_default_name(pass, &p.x),
        _ => None,
    }
}

fn types_entry_is_untyped(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(tv) = info.types.get(&expr.id()) else {
        return false;
    };
    is_untyped(&artifacts.types, tv.typ)
}

/// True when RHS is built from untyped const objects / literals (object type
/// still untyped), ignoring assignment-context Types typing of the expr.
fn rhs_names_untyped_const(pass: &Pass<'_>, expr: &Expr) -> bool {
    match unparen(expr) {
        Expr::BasicLit(lit) => matches!(
            lit.kind,
            Some(
                Token::INT
                    | Token::FLOAT
                    | Token::IMAG
                    | Token::CHAR
                    | Token::STRING
            )
        ),
        Expr::Ident(id) if id.name == "true" || id.name == "false" => true,
        Expr::Ident(id) => const_object_is_untyped(pass, id.id()),
        Expr::SelectorExpr(sel) => const_object_is_untyped(pass, sel.sel.id()),
        Expr::UnaryExpr(u) if matches!(u.op, Token::ADD | Token::SUB | Token::XOR) => {
            rhs_names_untyped_const(pass, &u.x)
        }
        Expr::BinaryExpr(b)
            if matches!(
                b.op,
                Token::ADD
                    | Token::SUB
                    | Token::MUL
                    | Token::QUO
                    | Token::REM
                    | Token::AND
                    | Token::OR
                    | Token::XOR
                    | Token::SHL
                    | Token::SHR
            ) =>
        {
            rhs_names_untyped_const(pass, &b.x) && rhs_names_untyped_const(pass, &b.y)
        }
        Expr::ParenExpr(p) => rhs_names_untyped_const(pass, &p.x),
        _ => false,
    }
}

fn const_object_is_untyped(pass: &Pass<'_>, node_id: u32) -> bool {
    use guff_types::arena::ObjectData;

    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(&obj) = info.uses.get(&node_id) else {
        return false;
    };
    let ObjectData::Const(c) = artifacts.objects.get(obj) else {
        return false;
    };
    is_untyped(&artifacts.types, c.typ())
}

fn untyped_basic_default(
    arena: &guff_types::arena::TypeArena,
    typ: guff_types::TypeId,
) -> Option<&'static str> {
    if !is_untyped(arena, typ) {
        return None;
    }
    match arena.get(typ) {
        TypeData::Basic(b) => match b.kind() {
            BasicKind::UntypedBool => Some("bool"),
            BasicKind::UntypedInt => Some("int"),
            BasicKind::UntypedRune => Some("rune"),
            BasicKind::UntypedFloat => Some("float64"),
            BasicKind::UntypedComplex => Some("complex128"),
            BasicKind::UntypedString => Some("string"),
            _ => None,
        },
        _ => None,
    }
}

fn const_ident_default(pass: &Pass<'_>, node_id: u32) -> Option<&'static str> {
    use guff_types::arena::ObjectData;

    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let &obj = info.uses.get(&node_id)?;
    let ObjectData::Const(c) = artifacts.objects.get(obj) else {
        return None;
    };
    untyped_basic_default(&artifacts.types, c.typ())
}

fn max_default_name(a: &'static str, b: &'static str) -> &'static str {
    const ORDER: &[&str] = &["int", "rune", "float64", "complex128", "bool", "string"];
    let ai = ORDER.iter().position(|&x| x == a).unwrap_or(0);
    let bi = ORDER.iter().position(|&x| x == b).unwrap_or(0);
    if ai >= bi {
        a
    } else {
        b
    }
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
