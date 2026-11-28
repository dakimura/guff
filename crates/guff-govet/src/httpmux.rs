//! `httpmux` — Go 1.22 enhanced `ServeMux` patterns in an older module.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/httpmux` (x/tools v0.52.0,
//! the version golangci-lint v2.15.0 links).
//!
//! Three things decide whether anything is reported at all, and each one is
//! easy to get subtly wrong:
//!
//! - The version gate reads `pass.Pkg.GoVersion()` — the **package's** version
//!   (the module's `go` line), not a file's `//go:build` line — and goes
//!   through `semver.Compare(v, "v1.21") > 0`. `go 1.21` and `go 1.21.0` run;
//!   `go 1.21.1` already counts as "after 1.21" and silences the analyzer. An
//!   unknown version ("") also counts as "after" (upstream: "Maybe the
//!   stdlib?").
//! - The package must import `net/http` *directly*.
//! - The callee is the **static** callee: `http.Handle` / `http.HandleFunc`,
//!   or a `Handle` / `HandleFunc` method of package `net/http` whose declared
//!   receiver is `*ServeMux`. The receiver is the method's, not the operand's,
//!   so a call on an addressable `http.ServeMux` value or through an embedded
//!   `*http.ServeMux` is reported just the same.

use std::sync::OnceLock;

use guff::doc::comment::{is_digit, is_letter};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{callee_full_name, expr_to_string};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::stdversion::package_go_version;

const MESSAGE: &str =
    "possible enhanced ServeMux pattern used with Go version before 1.22 (update go.mod file?)";

/// `isServeMuxRegisterCall`, by the callee's `types.Func.FullName()`.
fn is_serve_mux_register(full_name: &str) -> bool {
    matches!(
        full_name,
        "net/http.Handle"
            | "net/http.HandleFunc"
            | "(*net/http.ServeMux).Handle"
            | "(*net/http.ServeMux).HandleFunc"
    )
}

/// `/\{[_\pL][_\pL\p{Nd}]*(\.\.\.)?\}` matches somewhere in `pat`.
///
/// DEFERRED: the L / Nd tables were generated from go1.26.5 (Unicode 15.0.0);
/// golangci-lint v2.15.0 is built with go1.27 (Unicode 17.0.0), so a wildcard
/// name made of a letter assigned in Unicode 16 or 17 is reported upstream and
/// not here. The fix is regenerating the shared tables (which also moves
/// gofmt's doc-comment parsing and SA1001's template lexer), not a local copy.
fn has_wildcard(pat: &str) -> bool {
    let mut rest = pat;
    while let Some(i) = rest.find("/{") {
        let after = &rest[i + 2..];
        let mut chars = after.char_indices();
        if let Some((_, first)) = chars.next() {
            if first == '_' || is_letter(first) {
                let mut end = after.len();
                for (j, c) in chars {
                    if !(c == '_' || is_letter(c) || is_digit(c)) {
                        end = j;
                        break;
                    }
                }
                let tail = &after[end..];
                if tail.starts_with('}') || tail.starts_with("...}") {
                    return true;
                }
            }
        }
        rest = &rest[i + 1..];
    }
    false
}

/// `likelyEnhancedPattern`.
fn likely_enhanced_pattern(pat: &str) -> bool {
    pat.contains(' ') || has_wildcard(pat)
}

/// A non-empty run of ASCII digits without a leading zero — the numeric
/// identifiers `semver.IsValid` accepts.
fn semver_number(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    s.parse().ok()
}

/// `goVersionAfter121(goVersion(pass.Pkg))`.
///
/// `versionFromGoVersion` turns `go1.21` into `v1.21.0`, `go1.21.3` into
/// `v1.21.3` and `go1.22rc1` into `v1.22.0-rc.1`; anything it cannot parse
/// becomes "", which `semver.Compare` ranks below every valid version — so
/// the analyzer *runs*. Only the literal "" from `GoVersion()` skips it.
fn go_version_after_121(go_version: &str) -> bool {
    if go_version.is_empty() {
        return true;
    }
    if go_version == "go1" {
        return false; // v1.0.0
    }
    // ^go(\d+\.\d+)(\.\d+|)((beta|rc)(\d+))?$
    let Some(rest) = go_version.strip_prefix("go") else {
        return false;
    };
    let digits_end = |s: &str| s.bytes().position(|b| !b.is_ascii_digit()).unwrap_or(s.len());
    let n = digits_end(rest);
    let (major, rest) = rest.split_at(n);
    let Some(rest) = rest.strip_prefix('.') else {
        return false;
    };
    let n = digits_end(rest);
    let (minor, mut rest) = rest.split_at(n);
    let mut patch = "0";
    if let Some(r) = rest.strip_prefix('.') {
        let n = digits_end(r);
        if n == 0 {
            return false;
        }
        patch = &r[..n];
        rest = &r[n..];
    }
    if !rest.is_empty() {
        let Some(num) = rest.strip_prefix("beta").or_else(|| rest.strip_prefix("rc")) else {
            return false;
        };
        if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        // A leading zero makes the whole semver string invalid, and an
        // invalid version compares below "v1.21".
        if semver_number(num).is_none() {
            return false;
        }
    }
    if major.is_empty() || minor.is_empty() {
        return false;
    }
    let (Some(major), Some(minor), Some(patch)) =
        (semver_number(major), semver_number(minor), semver_number(patch))
    else {
        return false;
    };
    // A prerelease sorts below its release, so it can only be "after" v1.21.0
    // when its own triple already is: the suffix never changes the answer.
    (major, minor, patch) > (1, 21, 0)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if go_version_after_121(&package_go_version(pass)) {
        return Ok(None);
    }
    if !pass.pkg().imports.contains_key("net/http") {
        return Ok(None);
    }

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "httpmux requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if !callee_full_name(pass, call).is_some_and(|f| is_serve_mux_register(&f)) {
            return;
        }
        let Some(arg) = call.args.first() else {
            return;
        };
        if expr_to_string(pass, arg).is_some_and(|pat| likely_enhanced_pattern(&pat)) {
            // `ReportRangef(call.Args[0], …)`: starts at the pattern argument.
            pending.push(arg.pos().0 as u32);
        }
    });

    for pos in pending {
        pass.reportf(pos, MESSAGE);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "httpmux",
        doc: "report using Go 1.22 enhanced ServeMux patterns in older Go versions",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/httpmux",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_gate_matches_semver_compare() {
        for v in ["go1.21", "go1.21.0", "go1.20", "go1.21rc1", "go1.22rc01", "go1", "go1.0", "1.22", "go1.022", "garbage"] {
            assert!(!go_version_after_121(v), "{v} should run");
        }
        for v in ["", "go1.21.1", "go1.22", "go1.22.0", "go1.22rc1", "go1.22beta2", "go2.0"] {
            assert!(go_version_after_121(v), "{v} should skip");
        }
    }

    #[test]
    fn wildcard_regexp() {
        for p in ["/{a}/b/", "example.com/c/{d}", "/{x...}", "/{_}", "/{é1}", "/{/{a}"] {
            assert!(has_wildcard(p), "{p}");
        }
        for p in ["/", "/{/a/}", "/{1}", "/x{a}", "/{a", "/{a..}", "/{a-b}", "{a}"] {
            assert!(!has_wildcard(p), "{p}");
        }
    }
}
