//! SA5012 — odd-sized slice passed to function expecting even size.
//!
//! Simplified port of `honnef.co/go/tools/staticcheck/sa5012`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_new_replacer(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    if let Some(arg) = call.args.first() {
        if let Some(n) = composite_lit_len(ctx, arg.value) {
            if n % 2 != 0 {
                call.args[0].invalid(format!(
                    "argument \"oldnew\" is expected to have even number of elements, but has {n} elements"
                ));
                return;
            }
        }
    }
    if call.args.len() % 2 != 0 {
        call.invalid(format!(
            "argument \"oldnew\" is expected to have even number of elements, but has {} elements",
            call.args.len()
        ));
    }
}

fn composite_lit_len(ctx: &CallContext<'_>, value: callcheck::SsaValue) -> Option<usize> {
    let v = callcheck::flatten_ssa_value(ctx.caller, value.value());
    let guff_ssa::value::Value::Instr(iid) = v else {
        return None;
    };
    match ctx.caller.instrs.get(iid) {
        guff_ssa::instr::InstrData::MakeSlice(ms) => {
            let len = ms.len?;
            let n = callcheck::extract_const_int(ctx.prog, ctx.caller, callcheck::SsaValue::new(len))?;
            Some(n as usize)
        }
        _ => None,
    }
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([(
            "strings.NewReplacer",
            check_new_replacer as callcheck::CheckFn,
        )])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA5012 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa5012_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5012",
        doc: "passing odd-sized slice to function expecting even size",
        url: "https://staticcheck.dev/docs/checks/#SA5012",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5012_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5012_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
