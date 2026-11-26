//! SA4015 — calling math.Ceil/Floor on converted integers is pointless.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::instr::{Convert, InstrData};
use guff_ssa::value::Value;
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;

fn is_converted_from_int(ctx: &CallContext<'_>, arg: &callcheck::Argument) -> bool {
    let v = callcheck::flatten_ssa_value(ctx.caller, arg.value.value());
    let Value::Instr(iid) = v else { return false };
    // Upstream asks for an `ir.Convert` — a *representation-changing*
    // conversion. `int -> float64` is exactly that. Matching `ChangeType`
    // instead (which go/ssa reserves for value-preserving renames) made this
    // arm dead, and the AST fallback that stood in for it fired on a plain
    // literal, which upstream never reports: `math.Ceil(1)` has no conversion
    // at all, the constant is already `float64`.
    let InstrData::Convert(Convert { x, .. }) = ctx.caller.instrs.get(iid) else { return false };
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
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA4015 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa4015_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4015",
        doc: "calling math.Ceil on floats converted from integers doesn't do anything useful",
        url: "https://staticcheck.dev/docs/checks/#SA4015",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
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
