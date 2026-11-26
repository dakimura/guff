//! SA1017 — unbuffered channel passed to `os/signal.Notify`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1017`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_notify_channel(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.first() else {
        return;
    };
    if callcheck::is_unbuffered_make_chan(ctx.prog, ctx.caller, arg.value) {
        call.args[0].invalid("the channel used with signal.Notify should be buffered");
    }
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([(
            "os/signal.Notify",
            check_notify_channel as callcheck::CheckFn,
        )])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1017 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1017_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1017",
        doc: "channels used with os/signal.Notify should be buffered",
        url: "https://staticcheck.dev/docs/checks/#SA1017",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1017 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1017_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1017_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
