//! SA5009 — invalid Printf call.
//!
//! Simplified port of `honnef.co/go/tools/staticcheck/sa5009`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check(call: &mut Call<'_>, ctx: &CallContext<'_>, format_idx: usize, args_start: usize) {
    let Some(fmt_arg) = call.args.get(format_idx) else {
        return;
    };
    let Some(format) =
        callcheck::extract_const_string(ctx.prog, ctx.caller, fmt_arg.value)
    else {
        return;
    };
    let nargs = call.args.len().saturating_sub(args_start);
    match check_format(&format, nargs) {
        Ok(()) => {}
        Err(msg) => call.args[format_idx].invalid(msg),
    }
}

fn check_format(format: &str, nargs: usize) -> Result<(), String> {
    // 1-based, matching Go's fmt explicit indices (`%[1]v`).
    let mut next_arg = 1usize;
    let mut max_used = 0usize;
    let bytes = format.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        if i + 1 >= bytes.len() {
            break;
        }
        if bytes[i + 1] == b'%' {
            i += 2;
            continue;
        }
        i += 1;
        let mut explicit_index: Option<usize> = None;
        // Explicit index: %[n]
        if i < bytes.len() && bytes[i] == b'[' {
            i += 1;
            let start = i;
            while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] != b']' || i == start {
                return Err("couldn't parse format string".into());
            }
            let idx: usize = std::str::from_utf8(&bytes[start..i])
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| "couldn't parse format string".to_string())?;
            if idx == 0 {
                return Err("couldn't parse format string".into());
            }
            explicit_index = Some(idx);
            i += 1; // ]
        }
        // Printf flags: # 0 + - ' ' (must skip before width `*`).
        while i < bytes.len() && matches!(bytes[i], b'#' | b'0' | b'+' | b'-' | b' ') {
            i += 1;
        }
        // Width `*` consumes an arg.
        if i < bytes.len() && bytes[i] == b'*' {
            let used = explicit_index.unwrap_or(next_arg);
            max_used = max_used.max(used);
            next_arg = used + 1;
            explicit_index = None; // width index doesn't carry to the verb
            i += 1;
        }
        while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            if i < bytes.len() && bytes[i] == b'*' {
                let used = explicit_index.unwrap_or(next_arg);
                max_used = max_used.max(used);
                next_arg = used + 1;
                explicit_index = None;
                i += 1;
            }
            while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                i += 1;
            }
        }
        if i >= bytes.len() {
            return Err("couldn't parse format string".into());
        }
        let verb = bytes[i] as char;
        i += 1;
        if verb == '%' {
            continue;
        }
        let used = explicit_index.unwrap_or(next_arg);
        max_used = max_used.max(used);
        next_arg = used + 1;
    }
    if max_used != nargs {
        return Err(format!(
            "Printf call needs {max_used} args but has {nargs} args"
        ));
    }
    Ok(())
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            ("fmt.Errorf", check0 as callcheck::CheckFn),
            ("fmt.Printf", check0 as callcheck::CheckFn),
            ("fmt.Sprintf", check0 as callcheck::CheckFn),
            ("fmt.Fprintf", check1 as callcheck::CheckFn),
        ])
    })
}

fn check0(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check(call, ctx, 0, 1);
}

fn check1(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check(call, ctx, 1, 2);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA5009 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa5009_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5009",
        doc: "invalid Printf call",
        url: "https://staticcheck.dev/docs/checks/#SA5009",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5009_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5009_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn format_arg_count() {
        assert!(check_format("%s %d", 2).is_ok());
        assert!(check_format("%s", 2).is_err());
        // Explicit indices reuse args (`%[1]q … %[1]q … %q` needs 2).
        assert!(check_format(
            r#"/explore?left={"datasource":%[1]q,"queries":[{"datasource":%[1]q,"expr":%q}],"range":{}}"#,
            2
        )
        .is_ok());
        assert!(check_format("%[2]s %[1]s", 2).is_ok());
        assert!(check_format("%[2]s", 1).is_err());
        // Width `*` after flags (`%-*s`) consumes an extra arg.
        assert!(check_format("%-*s %-*s %s\n", 5).is_ok());
        assert!(check_format("%-*s", 1).is_err());
    }
}
