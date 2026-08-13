//! S1009 — omit redundant nil check on slices, maps, and channels.
//!
//! Port of `honnef.co/go/tools/simple/s1009`.
//!
//! **Parentheses.** Upstream states this check as a `pattern` query, and
//! `pattern.match` strips `*ast.ParenExpr` at every recursion (before binding),
//! so `f((x))` matches wherever `f(x)` does. This port descends by hand, so
//! every descent has to `unparen` — `compat/fuzz.py`'s `paren` mutation found
//! nine S-checks going quiet on a parenthesized subexpression at once
//! (COMPAT-HARDENING §4, 2026-08-13).

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, Expr};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to, is_integer_literal, is_nil, unparen};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_constant::{int64_val, Kind};
use guff_types::arena::{ObjectData, TypeData};

fn may_have_side_effects(expr: &Expr) -> bool {
    match expr {
        Expr::CallExpr(_) => true,
        Expr::UnaryExpr(u) => may_have_side_effects(&u.x),
        Expr::BinaryExpr(b) => may_have_side_effects(&b.x) || may_have_side_effects(&b.y),
        Expr::IndexExpr(i) => may_have_side_effects(&i.x) || may_have_side_effects(&i.index),
        Expr::SelectorExpr(s) => may_have_side_effects(&s.x),
        Expr::StarExpr(s) => may_have_side_effects(&s.x),
        Expr::ParenExpr(p) => may_have_side_effects(&p.x),
        Expr::SliceExpr(s) => {
            may_have_side_effects(&s.x)
                || s.low.as_ref().is_some_and(|e| may_have_side_effects(e))
                || s.high.as_ref().is_some_and(|e| may_have_side_effects(e))
                || s.max.as_ref().is_some_and(|e| may_have_side_effects(e))
        }
        _ => false,
    }
}

fn is_const_zero(pass: &Pass<'_>, expr: &Expr) -> Option<bool> {
    if is_integer_literal(pass, expr, 0) {
        return Some(true);
    }
    let Expr::Ident(ident) = unparen(expr) else {
        return None;
    };
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj_id = info.uses.get(&ident.id).copied()?;
    let ObjectData::Const(c) = artifacts.objects.get(obj_id) else {
        return None;
    };
    let val = c.val();
    if val.kind() != Kind::Int {
        return None;
    }
    Some(int64_val(val) == (0, true))
}

fn nil_check_type(pass: &Pass<'_>, expr: &Expr) -> Option<&'static str> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let tav = info.types.get(&expr.id())?;
    let u = tav.typ.underlying(&artifacts.types);
    match artifacts.types.get(u) {
        TypeData::Slice(_) => Some("nil slices"),
        TypeData::Map(_) => Some("nil maps"),
        TypeData::Chan(_) => Some("nil channels"),
        _ => None,
    }
}

fn same_expr(pass: &Pass<'_>, a: &Expr, b: &Expr) -> bool {
    // Unparenthesizing here is what makes `(x) != nil && len(x) > 0` compare
    // equal, and it is also why there is no `ParenExpr` arm below: neither side
    // can still be one.
    let (a, b) = (unparen(a), unparen(b));
    if std::mem::discriminant(a) != std::mem::discriminant(b) {
        return false;
    }
    match (a, b) {
        (Expr::Ident(x), Expr::Ident(y)) => {
            if x.name != y.name {
                return false;
            }
            if x.id != 0 && x.id == y.id {
                return true;
            }
            if let Some(info) = pass.types_info() {
                return info.uses.get(&x.id) == info.uses.get(&y.id);
            }
            true
        }
        (Expr::StarExpr(x), Expr::StarExpr(y)) => same_expr(pass, &x.x, &y.x),
        (Expr::SelectorExpr(x), Expr::SelectorExpr(y)) => {
            x.sel.name == y.sel.name && same_expr(pass, &x.x, &y.x)
        }
        (Expr::IndexExpr(x), Expr::IndexExpr(y)) => {
            same_expr(pass, &x.x, &y.x) && same_expr(pass, &x.index, &y.index)
        }
        _ => false,
    }
}

fn check_binary(pass: &Pass<'_>, outer: &BinaryExpr) -> Option<(u32, String)> {
    if outer.op != Token::LAND && outer.op != Token::LOR {
        return None;
    }
    let Expr::BinaryExpr(inner) = unparen(&outer.x) else {
        return None;
    };
    let Expr::BinaryExpr(rhs) = unparen(&outer.y) else {
        return None;
    };
    let Expr::CallExpr(len_call) = unparen(&rhs.x) else {
        return None;
    };
    if !is_call_to(pass, len_call, "len") || len_call.args.len() != 1 {
        return None;
    }
    if !same_expr(pass, &inner.x, &len_call.args[0]) {
        return None;
    }
    if !is_nil(pass, &inner.y) {
        return None;
    }

    let eq_nil = outer.op == Token::LOR;
    if eq_nil && inner.op != Token::EQL {
        return None;
    }
    if !eq_nil && inner.op != Token::NEQ {
        return None;
    }

    let is_zero = is_const_zero(pass, &rhs.y)?;
    if may_have_side_effects(&inner.x) {
        return None;
    }

    let valid_rhs = if eq_nil {
        match rhs.op {
            Token::EQL => is_zero,
            Token::LEQ => true,
            Token::LSS => !is_zero,
            _ => false,
        }
    } else {
        match rhs.op {
            Token::EQL => !is_zero,
            Token::GEQ => !is_zero,
            Token::NEQ => is_zero,
            Token::GTR => true,
            _ => false,
        }
    };
    if !valid_rhs {
        return None;
    }

    let nil_type = nil_check_type(pass, &inner.x)?;
    Some((
        outer.x.pos().0 as u32,
        format!("should omit nil check; len() for {nil_type} is defined as zero"),
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1009 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(BinaryExpr), pass.files(), |n| {
        let NodeRef::BinaryExpr(expr) = n else {
            return;
        };
        if let Some(diag) = check_binary(pass, expr) {
            pending.push(diag);
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1009_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1009",
        doc: "omit redundant nil check on slices, maps, and channels",
        url: "https://staticcheck.dev/docs/checks/#S1009",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1009 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1009_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1009_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
