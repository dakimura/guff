//! SA1002 — invalid format in `time.Parse`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1002`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::gostd;

fn check(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.first() else {
        return;
    };
    let Some(layout) = callcheck::extract_const_string(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    if let Some(msg) = validate_go_time_layout(&layout) {
        call.args[0].invalid(msg);
    }
}

/// The whole of upstream's check body: substitute, `time.Parse(s, s)`, report
/// `err.Error()` verbatim.
///
/// `_` and `Z` are rewritten first because neither element can parse the text it
/// formats (`_2` pads with a space, `Z07:00` prints a bare `Z` for UTC), so a
/// reference layout containing them would otherwise be reported as invalid.
/// With the port in [`gostd::time`] doing the parsing, a layout is invalid
/// exactly when Go says so, worded exactly as Go words it.
fn validate_go_time_layout(layout: &str) -> Option<String> {
    let layout = layout.replace('_', " ").replace('Z', "-");
    gostd::time::parse(&layout, &layout)
        .err()
        .map(|e| e.to_string())
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| HashMap::from([("time.Parse", check as callcheck::CheckFn)]))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1002 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1002_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1002",
        doc: "invalid format in time.Parse",
        url: "https://staticcheck.dev/docs/checks/#SA1002",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1002 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1002_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1002_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    /// Ground truth: `time.Parse(s, s)` after SA1002's own substitutions, run
    /// through Go. The exhaustive differential lives in `tests/gostd_time.rs`.
    #[test]
    fn layout_validation_matches_go_smoke_cases() {
        assert_eq!(
            validate_go_time_layout("12345").as_deref(),
            Some(r#"parsing time "12345" as "12345": cannot parse "" as "4""#),
        );
        assert_eq!(validate_go_time_layout("2006"), None);
        assert_eq!(validate_go_time_layout("2006-01-02"), None);
    }

    /// A layout with no std element at all is a literal that parses itself, so
    /// upstream stays silent. The pre-port heuristic reported it — a false
    /// positive on any string that merely looked unlike a date.
    #[test]
    fn layout_without_std_elements_is_silent() {
        assert_eq!(validate_go_time_layout("not-a-layout"), None);
        assert_eq!(validate_go_time_layout("hello"), None);
        assert_eq!(validate_go_time_layout(""), None);
    }

    /// The `_`→` ` and `Z`→`-` substitutions run before Parse, so `Z07:00`
    /// reaches it as `-07:00` and `_2` as ` 2`.
    #[test]
    fn substitutions_run_before_parse() {
        assert_eq!(validate_go_time_layout("Z07:00"), None);
        assert_eq!(validate_go_time_layout("2006-01-02T15:04:05Z07:00"), None);
    }
}
