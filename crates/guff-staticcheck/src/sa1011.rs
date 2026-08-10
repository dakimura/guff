//! SA1011 — invalid UTF-8 in `strings` cutset / character-set arguments.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1011`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.get(1) else {
        return;
    };
    // Bytes, and only bytes: this check *is* the question "are these bytes
    // valid UTF-8?", so asking it of a Rust `String` — which is valid by
    // construction — made SA1011 unable to fire at all. Its unit test passed
    // throughout, because it called the validator directly.
    let Some(s) = callcheck::extract_const_bytes(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    if !is_valid_utf8_bytes(&s) {
        call.args[1].invalid("argument is not a valid UTF-8 encoded string");
    }
}

/// Validates a byte sequence as UTF-8 (Go `utf8.ValidString` semantics).
pub(crate) fn is_valid_utf8_bytes(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            ("strings.IndexAny", check as callcheck::CheckFn),
            ("strings.LastIndexAny", check as callcheck::CheckFn),
            ("strings.ContainsAny", check as callcheck::CheckFn),
            ("strings.Trim", check as callcheck::CheckFn),
            ("strings.TrimLeft", check as callcheck::CheckFn),
            ("strings.TrimRight", check as callcheck::CheckFn),
        ])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1011 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1011_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1011",
        doc: "various methods in the strings package expect valid UTF-8, but invalid input is provided",
        url: "https://staticcheck.dev/docs/checks/#SA1011",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1011 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1011_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1011_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn utf8_validation_matches_go_bytes() {
        assert!(!is_valid_utf8_bytes(&[0xff]));
        assert!(!is_valid_utf8_bytes(&[0x80]));
        assert!(!is_valid_utf8_bytes(&[0xc3])); // truncated sequence
        assert!(is_valid_utf8_bytes(b"abc"));
        assert!(is_valid_utf8_bytes("日本語".as_bytes()));
    }
}
