//! Port of [`github.com/stbenjam/no-sprintf-host-port`](https://github.com/stbenjam/no-sprintf-host-port)
//! (golangci-lint wrapper in `pkg/golinters/nosprintfhostport`).
//!
//! Detects `fmt.Sprintf` used to build `scheme://host:port` URLs; prefer
//! `net.JoinHostPort`.

use std::sync::OnceLock;

use guff::ast::{BasicLit, CallExpr, Expr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn as_string_lit(expr: &Expr) -> Option<&BasicLit> {
    match expr {
        Expr::BasicLit(lit) if lit.kind == Some(Token::STRING) => Some(lit),
        _ => None,
    }
}

fn unquote_raw(lit: &str) -> Option<String> {
    if lit.len() < 2 {
        return None;
    }
    let quote = lit.as_bytes()[0];
    if (quote == b'"' || quote == b'`') && lit.as_bytes()[lit.len() - 1] == quote {
        Some(lit[1..lit.len() - 1].to_string())
    } else {
        None
    }
}

fn get_call_pkg_func(call: &CallExpr) -> Option<(&str, &str)> {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return None;
    };
    let Expr::Ident(pkg) = sel.x.as_ref() else {
        return None;
    };
    Some((pkg.name.as_str(), sel.sel.name.as_str()))
}

fn is_scheme_char(c: char, first: bool) -> bool {
    if first {
        c.is_ascii_alphabetic()
    } else {
        c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')
    }
}

/// Match `^[a-zA-Z][a-zA-Z0-9+-.]*://%s:[^@]*$`.
fn matches_no_auth(fs: &str) -> bool {
    let Some(rest) = split_scheme(fs) else {
        return false;
    };
    let Some(after_host) = rest.strip_prefix("%s:") else {
        return false;
    };
    !after_host.contains('@')
}

/// Match `^[a-zA-Z][a-zA-Z0-9+-.]*://[^/]*@%s:.*$`.
fn matches_with_auth(fs: &str) -> bool {
    let Some(rest) = split_scheme(fs) else {
        return false;
    };
    let Some(at) = rest.find('@') else {
        return false;
    };
    if rest[..at].contains('/') {
        return false;
    }
    rest[at + 1..].starts_with("%s:")
}

fn split_scheme(fs: &str) -> Option<&str> {
    let mut chars = fs.char_indices();
    let (_, first) = chars.next()?;
    if !is_scheme_char(first, true) {
        return None;
    }
    let mut end = first.len_utf8();
    for (i, c) in chars {
        if !is_scheme_char(c, false) {
            break;
        }
        end = i + c.len_utf8();
    }
    fs[end..].strip_prefix("://")
}

fn check_sprintf(call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Some(("fmt", "Sprintf")) = get_call_pkg_func(call) else {
        return;
    };
    let Some(fs_lit) = call.args.first().and_then(as_string_lit) else {
        return;
    };
    let Some(fs) = unquote_raw(&fs_lit.value) else {
        return;
    };

    let matched_no_auth = matches_no_auth(&fs);
    let matched_with_auth = matches_with_auth(&fs);
    if !matched_no_auth && !matched_with_auth {
        return;
    }

    // Upstream: without basic auth, allow when host arg is a literal without ':'.
    if matched_no_auth && call.args.len() <= 3 {
        if let Some(arg) = call.args.get(1).and_then(as_string_lit) {
            if let Some(host) = unquote_raw(&arg.value) {
                if !host.contains(':') {
                    return;
                }
            }
        }
    }

    pending.push((
        call.pos().0 as u32,
        "host:port in url should be constructed with net.JoinHostPort and not directly with fmt.Sprintf"
            .into(),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "nosprintfhostport requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::CallExpr(call) = n {
                check_sprintf(call, &mut pending);
            }
            true
        });
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "nosprintfhostport",
        doc: "Checks for misuse of Sprintf to construct a host with port in a URL.",
        url: "https://github.com/stbenjam/no-sprintf-host-port",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
