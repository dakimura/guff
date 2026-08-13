//! `var-declaration` — drop redundant type or zero-value from var declarations.

use guff::ast::{Expr, Ident, Spec, ValueSpec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::predicates::is_untyped;

use crate::failure::Failure;
use crate::util::{is_blank, is_ident, is_interface_type_expr, type_of, unparen};

/// This rule cannot ride `shared_walk` (which never prunes): upstream's
/// visitor returns `nil` from **every** path of its `*ast.ValueSpec` case, so
/// the walk never descends into a declaration's value. A `var` inside a
/// function literal that initializes another `var` is therefore invisible to
/// upstream — which is most of a ginkgo suite, where everything lives under
/// `var _ = Describe("…", func() { … })`. The non-var/non-const `GenDecl` case
/// returns `nil` as well; here that falls out of pruning at every `GenDecl`,
/// since guff reads a var declaration's specs inline instead of walking into
/// them.
pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::GenDecl(g)) = n else {
                return true;
            };
            if g.tok == Some(Token::VAR) {
                for spec in &g.specs {
                    if let Spec::ValueSpec(vs) = spec {
                        check_value_spec(pass, vs, &mut failures);
                    }
                }
            }
            false
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
    // Upstream's gate above `IsUntypedConst` is
    // `if !validType(lhsTyp) || !validType(rhsTyp) { return }`, commented
    // "Type checking failed (often due to missing imports)" — and for revive
    // that is not a rare failure. It type-checks with its own `lint.Package`,
    // so *any* operand reaching into an import comes back invalid and the rule
    // bails before it ever asks about constants.
    //
    // Measured against golangci-lint 2.12.2, one var block per shape:
    // `local1 + local1`, `localFunc()` and `localVar` are all reported, while
    // `sub.EscapingKey`, `sub.Func()` and `sub.Var` are all silent. So the line
    // is drawn at "does the right-hand side reach into another package", not at
    // constness — `config/config.go:579` in prometheus
    // (`model.EscapingKey + "=" + model.AllowUTF8`) is the silent side.
    //
    // The gate this replaces was removed wholesale on 2026-08-13 for firing on
    // every literal. It was over-broad, but it was not baseless: it approximated
    // this same behaviour through the one shape that had been noticed then.
    if rhs_refers_to_other_package(pass, rhs) {
        return;
    }
    // Upstream has exactly one gate here: `IsUntypedConst(rhs)` re-evaluates the
    // right-hand side *outside* assignment context, and the finding is dropped
    // only when the declared type is not the constant's default type. So
    // `var b int = 1` is reported and `var e int64 = 1` is not.
    //
    // guff used to carry a second gate — "the RHS names an untyped const but its
    // `Types` entry is typed, so skip" — which has no upstream counterpart and
    // fired on *every* literal, because a literal's `Types` entry always carries
    // the type the assignment gave it. That silenced the rule's entire common
    // case: `var a string = "x"`, `var b int = 1`, `var c float64 = 1.5`,
    // `var d bool = true` were all missed, and only a non-constant right-hand
    // side was ever reported. Found by `compat/fuzz.py`'s `littype` mutation,
    // which writes exactly this form from a `:=` (COMPAT-HARDENING §4,
    // 2026-08-13).
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

/// Reports whether any identifier in `expr` is an import name, i.e. whether the
/// expression reaches into another package. See the call site for the measured
/// upstream behaviour this stands in for.
fn rhs_refers_to_other_package(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut found = false;
    walk::preorder(walk::expr_ref(expr), |n| {
        let NodeRef::Ident(ident) = n else {
            return true;
        };
        let obj = info
            .defs
            .get(&ident.id)
            .and_then(|o| *o)
            .or_else(|| info.uses.get(&ident.id).copied());
        if obj.is_some_and(|o| matches!(artifacts.objects.get(o), ObjectData::PkgName(_))) {
            found = true;
            return false;
        }
        true
    });
    found
}

/// Approximate revive's `File.IsUntypedConst`: detect RHS expressions that are
/// untyped constants (literals, named untyped consts, or ops over them) and
/// return their default type name (`"int"`, `"float64"`, …).
fn untyped_const_default_name(pass: &Pass<'_>, expr: &Expr) -> Option<&'static str> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;

    // Syntax and objects first, `Types` only as a fallback. Reading `Types`
    // first cannot answer this question: assignment context has already given
    // the operand the declared type, so `1` in `var e int64 = 1` comes back
    // `int64` and the comparison against the *default* type (`int`) can no
    // longer be made. Upstream sidesteps it by re-evaluating the expression in
    // a fresh context; here the answer is read off the literal's token kind and
    // the constant object's own type, which is the same information.
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
        // Not syntactically a constant: fall back to whatever the type checker
        // recorded, in case it is still untyped there.
        _ => {
            let info = pass.types_info()?;
            let tv = info.types.get(&expr.id())?;
            untyped_basic_default(&artifacts.types, tv.typ)
        }
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
