//! SA1000 — invalid regular expression.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1000` (callcheck + buildir).
//!
//! Upstream is `regexp.Compile(s)` and `err.Error()` verbatim, so the check is
//! really Go's `regexp/syntax` parser; that lives in [`gostd::regexp`]. What
//! used to be here instead was `regex-syntax` plus hand-written rewrites that
//! escaped non-quantifier braces and perl-class range endpoints — the two Go
//! accepts and Rust does not — and it agreed with Go on half the oracle corpus.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::gostd;

fn validate_go_regex(pattern: &str) -> Option<String> {
    // `Err(None)` is gostd::regexp declining past its recursion limit. Upstream
    // has no such state, so the only faithful thing to do with it is stay
    // quiet: SA1000 reports every error regexp.Compile returns, which leaves no
    // whitelist for a guff-only message to fall outside of.
    gostd::regexp::compile(pattern).err().flatten()
}

fn check(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.first() else {
        return;
    };
    let Some(pattern) = callcheck::extract_const_string(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    if let Some(msg) = validate_go_regex(&pattern) {
        call.args[0].invalid(msg);
    }
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            ("regexp.MustCompile", check as callcheck::CheckFn),
            ("regexp.Compile", check as callcheck::CheckFn),
            ("regexp.Match", check as callcheck::CheckFn),
            ("regexp.MatchReader", check as callcheck::CheckFn),
            ("regexp.MatchString", check as callcheck::CheckFn),
        ])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1000 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1000_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1000",
        doc: "invalid regular expression",
        url: "https://staticcheck.dev/docs/checks/#SA1000",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1000 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1000_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1000_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn validate_go_regex_reports_gos_text_verbatim() {
        assert_eq!(validate_go_regex("abc"), None);
        assert_eq!(
            validate_go_regex("foo("),
            Some("error parsing regexp: missing closing ): `foo(`".to_string())
        );
        assert_eq!(
            validate_go_regex("["),
            Some("error parsing regexp: missing closing ]: `[`".to_string())
        );
        // Go RE2 accepts literal `{…}` and `[\w-]` — must not FP (caddy).
        assert_eq!(validate_go_regex(r"{header\.([\w-]*)}"), None);
        assert_eq!(validate_go_regex(r"{re\.([\w-\.]*)}"), None);
        // Grafana cloud-monitoring wildcard escaper — Go accepts nested `[` in class.
        assert_eq!(validate_go_regex(r"[-\/^$+?.()|[\]{}]"), None);
    }
}

