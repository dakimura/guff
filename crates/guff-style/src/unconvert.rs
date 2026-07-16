//! Port of [`github.com/mdempsky/unconvert`](https://github.com/mdempsky/unconvert)
//! (golangci-lint wrapper in `pkg/golinters/unconvert`).
//!
//! Finds unnecessary explicit type conversions (`T(x)` where `x` already has
//! type `T`). Floating-point / complex conversions are kept by default (Go 1.9+
//! rounding / fusion); set `linters.settings.unconvert.fast-math: true` to
//! report those too.
//!
//! DEFERRED: full `-safe` context filtering (assign/call/return parent checks).
//! When `safe: true` we currently still use the default (non-safe) heuristics.

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, CallExpr, Expr, Ident, SelectorExpr, UnaryExpr};
use guff::position::NO_POS;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::ObjectData;
use guff_types::predicates::{identical, is_complex, is_float, is_untyped};
use guff_types::{OperandMode, TypeId};

use crate::options::UnconvertOptions;

fn unparen(e: &Expr) -> &Expr {
    let mut cur = e;
    while let Expr::ParenExpr(p) = cur {
        cur = &p.x;
    }
    cur
}

fn type_and_mode(pass: &Pass<'_>, expr: &Expr) -> Option<(TypeId, OperandMode)> {
    let info = pass.types_info()?;
    let tav = info.types.get(&expr.id())?;
    Some((tav.typ, tav.mode))
}

fn is_floating_point(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let u = typ.underlying(&artifacts.types);
    is_float(&artifacts.types, u) || is_complex(&artifacts.types, u)
}

fn types_identical(pass: &Pass<'_>, a: TypeId, b: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    identical(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        a,
        b,
    )
}

fn as_builtin_name(pass: &Pass<'_>, fun: &Expr) -> Option<String> {
    let fun = unparen(fun);
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj = match fun {
        Expr::Ident(id) => info.uses.get(&id.id).copied()?,
        Expr::SelectorExpr(sel) => info.uses.get(&sel.sel.id).copied()?,
        _ => return None,
    };
    match artifacts.objects.get(obj) {
        ObjectData::Builtin(b) => Some(b.name().to_string()),
        _ => None,
    }
}

fn is_untyped_value(pass: &Pass<'_>, expr: &Expr) -> bool {
    match unparen(expr) {
        Expr::BinaryExpr(b) => is_untyped_binary(pass, b),
        Expr::UnaryExpr(u) => is_untyped_unary(pass, u),
        Expr::BasicLit(_) => true,
        Expr::SelectorExpr(sel) => is_untyped_selector(pass, sel),
        Expr::Ident(id) => is_untyped_ident(pass, id),
        Expr::CallExpr(call) => is_untyped_call(pass, call),
        _ => false,
    }
}

fn is_untyped_binary(pass: &Pass<'_>, b: &BinaryExpr) -> bool {
    match b.op {
        Token::SHL | Token::SHR => is_untyped_value(pass, &b.x),
        Token::EQL | Token::NEQ | Token::LSS | Token::GTR | Token::LEQ | Token::GEQ => true,
        Token::ADD
        | Token::SUB
        | Token::MUL
        | Token::QUO
        | Token::REM
        | Token::AND
        | Token::OR
        | Token::XOR
        | Token::AndNot
        | Token::LAND
        | Token::LOR => is_untyped_value(pass, &b.x) && is_untyped_value(pass, &b.y),
        _ => false,
    }
}

fn is_untyped_unary(pass: &Pass<'_>, u: &UnaryExpr) -> bool {
    match u.op {
        Token::ADD | Token::SUB | Token::NOT | Token::XOR => is_untyped_value(pass, &u.x),
        _ => false,
    }
}

fn is_untyped_selector(pass: &Pass<'_>, sel: &SelectorExpr) -> bool {
    // Upstream walks via Sel Ident uses; package-qualified consts are rare for
    // untyped values — treat Sel as the use site.
    is_untyped_ident(pass, &sel.sel)
}

fn is_untyped_ident(pass: &Pass<'_>, id: &Ident) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(obj) = info.uses.get(&id.id).copied() else {
        return false;
    };
    match artifacts.objects.get(obj) {
        ObjectData::Nil(_) => true,
        ObjectData::Const(c) => is_untyped(&artifacts.types, c.typ()),
        _ => false,
    }
}

fn is_untyped_call(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(name) = as_builtin_name(pass, &call.fun) else {
        return false;
    };
    match name.as_str() {
        "real" | "imag" => call
            .args
            .first()
            .is_some_and(|a| is_untyped_value(pass, a)),
        "complex" => {
            call.args.len() >= 2
                && is_untyped_value(pass, &call.args[0])
                && is_untyped_value(pass, &call.args[1])
        }
        _ => false,
    }
}

fn check_conversion(
    pass: &Pass<'_>,
    call: &CallExpr,
    options: &UnconvertOptions,
) -> Option<u32> {
    // Conversions have exactly one argument and no ellipsis.
    if call.args.len() != 1 || call.ellipsis != NO_POS {
        return None;
    }

    let (ft, fmode) = type_and_mode(pass, &call.fun)?;
    if fmode != OperandMode::TypeExpr {
        // Function call; not a conversion.
        return None;
    }
    let (at, _) = type_and_mode(pass, &call.args[0])?;
    if !types_identical(pass, ft, at) {
        return None;
    }
    if !options.fast_math && is_floating_point(pass, ft) {
        // Explicit float/complex conversions force rounding / prevent fusion.
        return None;
    }
    if is_untyped_value(pass, &call.args[0]) {
        // Workaround golang.org/issue/13061 — keep conversions of untyped values.
        return None;
    }
    // DEFERRED: options.safe parent-context filtering (isSafeContext).
    let _ = options.safe;

    Some(call.lparen.0 as u32)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "unconvert requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<UnconvertOptions>("unconvert")
        .copied()
        .unwrap_or_default();

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::CallExpr(call) = n {
                if let Some(pos) = check_conversion(pass, call, &options) {
                    pending.push(pos);
                }
            }
            true
        });
    }

    for pos in pending {
        pass.reportf(pos, "unnecessary conversion");
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "unconvert",
        doc: "Remove unnecessary type conversions",
        url: "https://github.com/mdempsky/unconvert",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn analyzer_graph_ok() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
