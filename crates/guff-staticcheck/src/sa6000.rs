//! SA6000 — regexp.Match in a loop should use regexp.Compile.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa6000`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{is_in_loop, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::ids::BlockId;
use guff_ssa::instr::InstrData;

fn check_match(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_in_loop(call, ctx, "regexp.Match");
}

fn check_match_reader(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_in_loop(call, ctx, "regexp.MatchReader");
}

fn check_match_string(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_in_loop(call, ctx, "regexp.MatchString");
}

fn check_in_loop(call: &mut Call<'_>, ctx: &CallContext<'_>, name: &str) {
    let Some(arg) = call.args.first() else {
        return;
    };
    if callcheck::extract_const_string(ctx.prog, ctx.caller, arg.value).is_none() {
        return;
    }
    if find_call_block(ctx.caller, call.common).is_some_and(|b| is_in_loop(ctx.caller, b)) {
        call.invalid(format!(
            "calling {name} in a loop has poor performance, consider using regexp.Compile"
        ));
    }
}

fn find_call_block(
    func: &guff_ssa::function::Function,
    common: &guff_ssa::instr::CallCommon,
) -> Option<BlockId> {
    for (bid, block) in func.blocks.iter() {
        for &iid in &block.instrs {
            if let InstrData::Call(c) = func.instrs.get(iid) {
                if std::ptr::eq(&c.call as *const _, common as *const _) {
                    return Some(bid);
                }
            }
        }
    }
    None
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            ("regexp.Match", check_match as callcheck::CheckFn),
            ("regexp.MatchReader", check_match_reader as callcheck::CheckFn),
            ("regexp.MatchString", check_match_string as callcheck::CheckFn),
        ])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA6000 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa6000_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA6000",
        doc: "using regexp.Match or related in a loop, should use regexp.Compile",
        url: "https://staticcheck.dev/docs/checks/#SA6000",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa6000_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa6000_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
