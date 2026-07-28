//! SA4015 — calling math.Ceil/Floor on converted integers is pointless.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::code::{expr_to_int, selector_name};
use guff_analysis::passes::{buildir, inspect};
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::instr::{ChangeType, InstrData};
use guff_ssa::value::Value;
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;

fn is_converted_from_int(ctx: &CallContext<'_>, arg: &callcheck::Argument) -> bool {
    let v = callcheck::flatten_ssa_value(ctx.caller, arg.value.value());
    let Value::Instr(iid) = v else { return false };
    let InstrData::ChangeType(ChangeType { x, .. }) = ctx.caller.instrs.get(iid) else { return false };
    let src_typ = callcheck::ssa_value_type(ctx.prog, ctx.caller, callcheck::SsaValue::new(*x));
    let arena = &ctx.prog.type_arena;
    let u = src_typ.underlying(arena);
    matches!(arena.get(u), TypeData::Basic(b) if matches!(b.kind(), BasicKind::Int | BasicKind::Int8 | BasicKind::Int16 | BasicKind::Int32 | BasicKind::Int64 | BasicKind::Uint | BasicKind::Uint8 | BasicKind::Uint16 | BasicKind::Uint32 | BasicKind::Uint64 | BasicKind::Uintptr))
}

fn pointless_int_math(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.first() else { return };
    if is_converted_from_int(ctx, arg) {
        let name = callcheck::call_target_name(ctx, call.common).unwrap_or_else(|| "math".into());
        call.invalid(format!("calling {name} on a converted integer is pointless"));
    }
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| HashMap::from([
        ("math.Ceil", pointless_int_math as callcheck::CheckFn),
        ("math.Floor", pointless_int_math as callcheck::CheckFn),
        ("math.IsNaN", pointless_int_math as callcheck::CheckFn),
        ("math.Trunc", pointless_int_math as callcheck::CheckFn),
        ("math.IsInf", pointless_int_math as callcheck::CheckFn),
    ]))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut pending = Vec::new();
    if pass.result_of::<buildir::BuildIrResult>(buildir::analyzer()).is_some() {
        callcheck::run(pass, rules());
    }
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4015 requires inspect analyzer".to_string())?
        .clone();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
            return;
        };
        if !matches!(sel.sel.name.as_str(), "Ceil" | "Floor" | "Trunc" | "IsNaN" | "IsInf") {
            return;
        }
        let Some(arg) = call.args.first() else {
            return;
        };
        if !arg_is_int_like(pass, arg) {
            return;
        }
        pending.push((
            call.lparen.0 as u32,
            format!("calling {} on a converted integer is pointless", sel.sel.name),
        ));
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn call_target(pass: &Pass<'_>, fun: &Expr) -> Option<String> {
    if let Expr::SelectorExpr(sel) = fun {
        if let Some(name) = selector_name(pass, sel) {
            return Some(name);
        }
        return Some(format!("{}.{}", "math", sel.sel.name));
    }
    None
}

fn arg_is_int_like(pass: &Pass<'_>, arg: &Expr) -> bool {
    if let Expr::BasicLit(lit) = arg {
        if !lit.value.contains('.') && lit.value.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '+') {
            return true;
        }
    }
    if expr_to_int(pass, arg).is_some() {
        return true;
    }
    if let Expr::CallExpr(CallExpr { fun, args, .. }) = arg {
        if let Expr::Ident(id) = fun.as_ref() {
            return id.name == "float64" && args.first().is_some_and(|a| arg_is_int_like(pass, a));
        }
    }
    false
}

fn sa4015_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4015",
        doc: "calling math.Ceil on floats converted from integers doesn't do anything useful",
        url: "https://staticcheck.dev/docs/checks/#SA4015",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer(), inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4015_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;
    #[test]
    fn sa4015_validates() { assert!(validate(&[analyzer()]).is_ok()); }
}
