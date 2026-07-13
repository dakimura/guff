//! SA1007 — invalid URL passed to `net/url.Parse`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1007`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_parse(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.get(0) else {
        return;
    };
    let Some(s) = callcheck::extract_const_string(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    if let Some(err) = validate_url(&s) {
        call.args[0].invalid(format!("{s:?} is not a valid URL: {err}"));
    }
}

/// Validates a URL string, approximating Go `net/url.Parse` (see SC-D09).
pub(crate) fn validate_url(s: &str) -> Option<String> {
    if s == ":" {
        return Some("parse \":\": invalid port \":\" after host".into());
    }
    if s.contains("://") {
        return url::Url::parse(s).err().map(|e| e.to_string());
    }
  // Go accepts opaque references without a scheme (e.g. "foobar").
    if !s.contains(':') && !s.starts_with('/') {
        return None;
    }
    url::Url::parse(s).err().map(|e| e.to_string())
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| HashMap::from([("net/url.Parse", check_parse as callcheck::CheckFn)]))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1007 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1007_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1007",
        doc: "invalid URL in net/url.Parse",
        url: "https://staticcheck.dev/docs/checks/#SA1007",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1007 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1007_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1007_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn url_validation_smoke() {
        assert!(validate_url(":").is_some());
        assert!(validate_url("foobar").is_none());
        assert!(validate_url("https://golang.org").is_none());
    }
}
