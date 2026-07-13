//! SA1018 — `strings.Replace` / `bytes.Replace` called with `n == 0`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1018`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_n_zero(call: &mut Call<'_>, ctx: &CallContext<'_>, name: &str, arg_idx: usize) {
    let Some(arg) = call.args.get_mut(arg_idx) else {
        return;
    };
    let Some(n) = callcheck::extract_const_int(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    if n == 0 {
        arg.invalid(format!(
            "calling {name} with n == 0 will return no results, did you mean -1?"
        ));
    }
}

fn check_strings_replace(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_n_zero(call, ctx, "strings.Replace", 3);
}

fn check_bytes_replace(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_n_zero(call, ctx, "bytes.Replace", 3);
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            ("strings.Replace", check_strings_replace as callcheck::CheckFn),
            ("bytes.Replace", check_bytes_replace as callcheck::CheckFn),
        ])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1018 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1018_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1018",
        doc: "strings.Replace called with n == 0, which does nothing",
        url: "https://staticcheck.dev/docs/checks/#SA1018",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1018 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1018_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1018_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
