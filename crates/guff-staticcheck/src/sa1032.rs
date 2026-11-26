//! SA1032 — wrong order of arguments to `errors.Is`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1032`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_errors_is(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    if call.args.len() != 2 {
        return;
    }

    let gx = callcheck::loaded_global(ctx.prog, ctx.caller, call.args[0].value);
    let Some(gx) = gx else {
        return;
    };
    let Some(pkgx) = callcheck::global_import_path(ctx.prog, gx) else {
        return;
    };
    if pkgx.is_empty() || pkgx == ctx.pkg_path {
        return;
    }

    if let Some(gy) = callcheck::loaded_global(ctx.prog, ctx.caller, call.args[1].value) {
        if let Some(pkgy) = callcheck::global_import_path(ctx.prog, gy) {
            if !pkgy.is_empty() && pkgy != ctx.pkg_path {
                return;
            }
        }
    }

    call.invalid("arguments have the wrong order");
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([("errors.Is", check_errors_is as callcheck::CheckFn)])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1032 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1032_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1032",
        doc: "wrong order of arguments to errors.Is",
        url: "https://staticcheck.dev/docs/checks/#SA1032",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1032 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1032_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1032_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
