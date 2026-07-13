//! SA1020 — invalid host:port pair for `net/http` listen helpers.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1020`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::callcheck::{self, Call, CallContext};
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

const MSG_INVALID_HOST_PORT: &str = "invalid port or service name in host:port pair";

fn check_listen_and_serve(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_valid_host_port(call, ctx, 0);
}

fn check_listen_and_serve_tls(call: &mut Call<'_>, ctx: &CallContext<'_>) {
    check_valid_host_port(call, ctx, 0);
}

fn check_valid_host_port(call: &mut Call<'_>, ctx: &CallContext<'_>, arg_idx: usize) {
    let Some(arg) = call.args.get(arg_idx) else {
        return;
    };
    let Some(addr) = callcheck::extract_const_string(ctx.prog, ctx.caller, arg.value) else {
        return;
    };
    if !valid_host_port(&addr) {
        call.args[arg_idx].invalid(MSG_INVALID_HOST_PORT);
    }
}

/// Port of Go `sa1020.ValidHostPort`.
pub(crate) fn valid_host_port(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    let Some(colon) = s.rfind(':') else {
        return false;
    };
    let (host, port) = s.split_at(colon);
    let port = &port[1..];
    if host.is_empty() && port.is_empty() {
        return false;
    }
    validate_port(port)
}

fn validate_port(s: &str) -> bool {
    if let Ok(n) = s.parse::<i64>() {
        return (0..=65535).contains(&n);
    }
    validate_service_name(s)
}

fn validate_service_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 15 {
        return false;
    }
    if s.starts_with('-') || s.ends_with('-') {
        return false;
    }
    if s.contains("--") {
        return false;
    }
    let mut has_letter = false;
    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            has_letter = true;
        } else if c.is_ascii_digit() {
            continue;
        } else {
            return false;
        }
    }
    has_letter
}

fn rules() -> &'static HashMap<&'static str, callcheck::CheckFn> {
    static RULES: OnceLock<HashMap<&'static str, callcheck::CheckFn>> = OnceLock::new();
    RULES.get_or_init(|| {
        HashMap::from([
            (
                "net/http.ListenAndServe",
                check_listen_and_serve as callcheck::CheckFn,
            ),
            (
                "net/http.ListenAndServeTLS",
                check_listen_and_serve_tls as callcheck::CheckFn,
            ),
        ])
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .is_none()
    {
        return Err("SA1020 requires buildir analyzer".into());
    }
    callcheck::run(pass, rules());
    Ok(None)
}

fn sa1020_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1020",
        doc: "using an invalid host:port pair with a net.Listen-related function",
        url: "https://staticcheck.dev/docs/checks/#SA1020",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1020 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1020_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1020_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn host_port_validation() {
        assert!(!valid_host_port("localhost:8080/"));
        assert!(!valid_host_port("localhost"));
        assert!(valid_host_port("localhost:8080"));
        assert!(valid_host_port(":8080"));
        assert!(valid_host_port(":http"));
        assert!(valid_host_port("localhost:http"));
        assert!(valid_host_port("local_host:8080"));
        assert!(valid_host_port(""));
    }
}
