//! SA1002 — invalid format in `time.Parse`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1002`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.first() else {
        return;
    };
    let Some(mut layout) = callcheck::extract_const_string(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    layout = layout.replace('_', " ");
    layout = layout.replace('Z', "-");
    if let Some(msg) = validate_go_time_layout(&layout) {
        call.args[0].invalid(msg);
    }
}

/// Mirrors `time.Parse(layout, layout)` from Go (see go-tools SA1002).
fn validate_go_time_layout(layout: &str) -> Option<String> {
    if go_time_layout_self_parse(layout).is_ok() {
        return None;
    }
    Some(format!("parsing time {layout:?} as {layout:?}"))
}

fn go_time_layout_self_parse(layout: &str) -> Result<(), ()> {
    if layout.is_empty() {
        return Err(());
    }
    if layout.chars().all(|c| c.is_ascii_digit()) {
        return if matches!(
            layout,
            "1" | "2" | "3" | "4" | "5" | "01" | "02" | "03" | "04" | "05" | "06" | "15" | "2006"
        ) {
            Ok(())
        } else {
            Err(())
        };
    }

    const TOKENS: &[&str] = &[
        "2006", "06", "January", "Jan", "Monday", "Mon", "01", "1", "02", "2", "_2", "15", "03",
        "3", "04", "4", "05", "5", "MST", "PM", "pm", "Z07", "-07", "002", "__2", "Kitchen",
        "RFC3339",
    ];
    if TOKENS.iter().any(|t| layout.contains(t)) {
        return Ok(());
    }
    if layout.contains(':') || layout.contains('-') || layout.contains('/') {
        if layout.contains('0') || layout.contains('1') || layout.contains('2') {
            return Ok(());
        }
    }
    Err(())
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

    #[test]
    fn layout_validation_matches_go_smoke_cases() {
        assert!(validate_go_time_layout("12345").is_some());
        assert!(validate_go_time_layout("2006").is_none());
        assert!(validate_go_time_layout("2006-01-02").is_none());
    }
}
