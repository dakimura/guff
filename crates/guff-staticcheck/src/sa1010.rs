//! SA1010 — `(*regexp.Regexp).FindAll*` called with `n == 0`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1010`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_n_zero(call: &mut Call<'_>, ctx: &CallContext<'_>, arg_idx: usize) {
    let Some(arg) = call.args.get_mut(arg_idx) else {
        return;
    };
    let Some(n) = callcheck::extract_const_int(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    if n == 0 {
        arg.invalid(
            "calling a FindAll method with n == 0 will return no results, did you mean -1?",
        );
    }
}

fn check_find_all(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_n_zero(call, ctx, 1);
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            (
                "(*regexp.Regexp).FindAll",
                check_find_all as callcheck::CheckFn,
            ),
            (
                "(*regexp.Regexp).FindAllIndex",
                check_find_all as callcheck::CheckFn,
            ),
            (
                "(*regexp.Regexp).FindAllString",
                check_find_all as callcheck::CheckFn,
            ),
            (
                "(*regexp.Regexp).FindAllStringIndex",
                check_find_all as callcheck::CheckFn,
            ),
            (
                "(*regexp.Regexp).FindAllStringSubmatch",
                check_find_all as callcheck::CheckFn,
            ),
            (
                "(*regexp.Regexp).FindAllStringSubmatchIndex",
                check_find_all as callcheck::CheckFn,
            ),
            (
                "(*regexp.Regexp).FindAllSubmatch",
                check_find_all as callcheck::CheckFn,
            ),
            (
                "(*regexp.Regexp).FindAllSubmatchIndex",
                check_find_all as callcheck::CheckFn,
            ),
        ])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1010 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1010_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1010",
        doc: "(*regexp.Regexp).FindAll called with n == 0, which will always return zero results",
        url: "https://staticcheck.dev/docs/checks/#SA1010",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1010 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1010_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1010_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
