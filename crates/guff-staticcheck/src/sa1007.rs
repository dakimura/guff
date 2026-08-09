//! SA1007 — invalid URL passed to `net/url.Parse`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1007`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::gostd;

fn check_parse(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.first() else {
        return;
    };
    let Some(s) = callcheck::extract_const_string(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    if let Some(msg) = report_for(&s) {
        call.args[0].invalid(msg);
    }
}

/// Upstream's whole check body: `url.Parse(s)`, reported as
/// `fmt.Sprintf("%q is not a valid URL: %s", s, err)`.
///
/// `%q` is `strconv.Quote`, not Rust's `{:?}` — the two differ on the single
/// quote and on every non-printable rune.
fn report_for(s: &str) -> Option<String> {
    let err = gostd::url::parse(s).err()?;
    Some(format!(
        "{} is not a valid URL: {err}",
        gostd::strconv::quote(s)
    ))
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

    /// Expectations are golangci-lint's own output; the exhaustive differential
    /// against `url.Parse` lives in `tests/gostd_url.rs`.
    #[test]
    fn url_validation_smoke() {
        assert_eq!(
            report_for(":").as_deref(),
            Some(r#"":" is not a valid URL: parse ":": missing protocol scheme"#),
        );
        // Go accepts a relative reference and an opaque scheme:path; the
        // WHATWG parser behind Rust's `url` crate rejects both.
        assert_eq!(report_for("foobar"), None);
        assert_eq!(report_for("mailto:a@b.c"), None);
        assert_eq!(report_for("https://golang.org"), None);
    }
}
