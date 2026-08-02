//! SA1000 — invalid regular expression.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1000` (callcheck + buildir).

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn validate_go_regex(pattern: &str) -> Option<String> {
    // Go's `regexp` treats `{`/`}` as literals unless they form a quantifier
    // (`{n}`, `{n,}`, `{n,m}`), and allows perl-class endpoints like `[\w-.]`.
    // Rust `regex-syntax` rejects both. Soften to Go-compatible form before
    // parsing so we only report patterns that are also invalid in Go (caddy
    // placeholder regexes).
    let softened = soften_go_regex_for_rust(pattern);
    let mut parser = regex_syntax::ast::parse::ParserBuilder::new()
        .octal(true)
        .build();
    match parser.parse(&softened) {
        Ok(_) => None,
        Err(err) => Some(format!("error parsing regexp: {err}")),
    }
}

/// Escape non-quantifier braces and soften Go-only char-class forms for Rust.
fn soften_go_regex_for_rust(pattern: &str) -> String {
    let braced = escape_non_quantifier_braces(pattern);
    soften_perl_hyphens_in_classes(&braced)
}

fn escape_non_quantifier_braces(pattern: &str) -> String {
    let bytes = pattern.as_bytes();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut i = 0;
    let mut in_class = false;
    let mut escaped = false;
    while i < bytes.len() {
        let b = bytes[i];
        if escaped {
            out.push(b as char);
            escaped = false;
            i += 1;
            continue;
        }
        if b == b'\\' {
            out.push('\\');
            escaped = true;
            i += 1;
            continue;
        }
        if !in_class && b == b'[' {
            in_class = true;
            out.push('[');
            i += 1;
            // Negation / leading `]` are literals in Go/RE2.
            if i < bytes.len() && bytes[i] == b'^' {
                out.push('^');
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b']' {
                out.push(']');
                i += 1;
            }
            continue;
        }
        if in_class {
            // Go RE2 treats `[` inside a class as a literal; Rust's parser may
            // start a nested class and then fail on patterns like `[…|[\]{}]`.
            if b == b'[' {
                out.push_str(r"\[");
                i += 1;
                continue;
            }
            if b == b']' {
                in_class = false;
            }
            out.push(b as char);
            i += 1;
            continue;
        }
        if b == b'{' {
            if let Some(end) = quantifier_end(bytes, i) {
                out.push_str(&pattern[i..=end]);
                i = end + 1;
            } else {
                out.push_str(r"\{");
                i += 1;
            }
            continue;
        }
        if b == b'}' {
            // Lone `}` is a literal in Go.
            out.push_str(r"\}");
            i += 1;
            continue;
        }
        out.push(b as char);
        i += 1;
    }
    out
}

/// `{n}`, `{n,}`, `{n,m}` starting at `open` (`bytes[open] == b'{'`).
fn quantifier_end(bytes: &[u8], open: usize) -> Option<usize> {
    if open >= bytes.len() || bytes[open] != b'{' {
        return None;
    }
    let mut i = open + 1;
    let mut saw_digit = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        saw_digit = true;
        i += 1;
    }
    if !saw_digit {
        return None;
    }
    if i < bytes.len() && bytes[i] == b'}' {
        return Some(i);
    }
    if i >= bytes.len() || bytes[i] != b',' {
        return None;
    }
    i += 1; // comma
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'}' {
        Some(i)
    } else {
        None
    }
}

/// Go allows `[\w-.]` / `[\w-\.]`; Rust rejects `\w` as a range endpoint.
/// Escape the hyphen after a perl class so the class stays a set of literals.
fn soften_perl_hyphens_in_classes(pattern: &str) -> String {
    let bytes = pattern.as_bytes();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut i = 0;
    let mut in_class = false;
    let mut escaped = false;
    while i < bytes.len() {
        let b = bytes[i];
        if escaped {
            out.push(b as char);
            escaped = false;
            i += 1;
            continue;
        }
        if b == b'\\' {
            // Perl class + `-` + next token inside a class → escape hyphen.
            if in_class
                && i + 2 < bytes.len()
                && matches!(bytes[i + 1], b'd' | b'D' | b's' | b'S' | b'w' | b'W')
                && bytes[i + 2] == b'-'
                && i + 3 < bytes.len()
                && bytes[i + 3] != b']'
            {
                out.push('\\');
                out.push(bytes[i + 1] as char);
                out.push_str(r"\-");
                i += 3;
                continue;
            }
            out.push('\\');
            escaped = true;
            i += 1;
            continue;
        }
        if !in_class && b == b'[' {
            in_class = true;
            out.push('[');
            i += 1;
            if i < bytes.len() && bytes[i] == b'^' {
                out.push('^');
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b']' {
                out.push(']');
                i += 1;
            }
            continue;
        }
        if in_class {
            if b == b'[' {
                out.push_str(r"\[");
                i += 1;
                continue;
            }
            if b == b']' {
                in_class = false;
            }
        }
        out.push(b as char);
        i += 1;
    }
    out
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
    fn validate_go_regex_flags_common_errors() {
        assert!(validate_go_regex("abc").is_none());
        assert!(validate_go_regex("foo(").is_some());
        assert!(validate_go_regex("[").is_some());
        // Go RE2 accepts literal `{…}` and `[\w-]` — must not FP (caddy).
        assert!(validate_go_regex(r"{header\.([\w-]*)}").is_none());
        assert!(validate_go_regex(r"{re\.([\w-\.]*)}").is_none());
        // Grafana cloud-monitoring wildcard escaper — Go accepts nested `[` in class.
        assert!(validate_go_regex(r"[-\/^$+?.()|[\]{}]").is_none());
    }
}
