//! SA1024 — a string cutset contains duplicate characters.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1024`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_constant::Kind;

fn check(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    let Some(arg) = call.args.get(1) else {
        return;
    };
    if !is_unique_string_cutset(ctx, arg.value) {
        call.args[1].invalid("cutset contains duplicate characters");
    }
}

fn is_unique_string_cutset(ctx: &CallContext<'_>, value: callcheck::SsaValue) -> bool {
    let Some(c) = callcheck::extract_const(ctx.prog, ctx.caller, value) else {
        return true;
    };
    let Some(val) = c.val.as_ref() else {
        return true;
    };
    if val.kind() != Kind::String {
        return true;
    }
    // Upstream converts the cutset with `[]rune(s)`, which turns each
    // ill-formed byte into U+FFFD — so two of them collide and the cutset is
    // *not* unique. `string_val_lossy` reproduces that, byte for byte.
    let s: Vec<char> = guff_constant::string_val_lossy(val).chars().collect();
    if s.len() < 2 {
        return true;
    }
    let mut sorted = s.clone();
    sorted.sort_unstable();
    for i in 1..sorted.len() {
        if sorted[i - 1] == sorted[i] {
            return false;
        }
    }
    true
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
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
        return Err("SA1024 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1024_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1024",
        doc: "a string cutset contains duplicate characters",
        url: "https://staticcheck.dev/docs/checks/#SA1024",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1024 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1024_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1024_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn cutset_uniqueness_logic() {
        assert!(!unique_cutset_chars("aba"));
        assert!(unique_cutset_chars("abc"));
        assert!(unique_cutset_chars("a"));
    }

    fn unique_cutset_chars(s: &str) -> bool {
        let mut chars: Vec<char> = s.chars().collect();
        if chars.len() < 2 {
            return true;
        }
        chars.sort_unstable();
        for i in 1..chars.len() {
            if chars[i - 1] == chars[i] {
                return false;
            }
        }
        true
    }
}
