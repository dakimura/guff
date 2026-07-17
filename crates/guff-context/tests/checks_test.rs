mod support;

use std::sync::Arc;

use guff_analysis::SettingsBag;
use guff_context::{bodyclose, fatcontext, noctx, BodycloseOptions};
use guff_runner::RunnerOptions;

#[test]
fn noctx_flags_http_new_request() {
    let dir = support::testdata("noctx");
    let pkg = support::typecheck_pkg("example.com/noctx", &dir.join("bad.go"));
    let messages = support::run_analyzer(noctx(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("NewRequest") && m.contains("NewRequestWithContext")),
        "{messages:?}"
    );
}

#[test]
fn noctx_allows_new_request_with_context() {
    let dir = support::testdata("noctx");
    let pkg = support::typecheck_pkg("example.com/noctx/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(noctx(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(noctx(), &pkg)
    );
}

#[test]
fn fatcontext_flags_nested_context_in_loop() {
    let dir = support::testdata("fatcontext");
    let pkg = support::typecheck_pkg("example.com/fatcontext", &dir.join("bad.go"));
    let messages = support::run_analyzer(fatcontext(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("nested context in loop")),
        "{messages:?}"
    );
}

#[test]
fn fatcontext_allows_shadowing_define() {
    let dir = support::testdata("fatcontext");
    let pkg = support::typecheck_pkg("example.com/fatcontext/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(fatcontext(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(fatcontext(), &pkg)
    );
}

#[test]
fn bodyclose_flags_missing_close() {
    let dir = support::testdata("bodyclose");
    let pkg = support::typecheck_pkg("example.com/bodyclose", &dir.join("bad.go"));
    let messages = support::run_analyzer(bodyclose(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("response body must be closed")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("response body must be closed"))
            .count()
            >= 3,
        "expected ≥3 diagnostics (missing + discarded + reassign): {messages:?}"
    );
}

#[test]
fn bodyclose_allows_closed_and_returned() {
    let dir = support::testdata("bodyclose");
    let pkg = support::typecheck_pkg("example.com/bodyclose/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(bodyclose(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(bodyclose(), &pkg)
    );
}

#[test]
fn bodyclose_check_consumption_requires_read() {
    let dir = support::testdata("bodyclose");
    let pkg = support::typecheck_pkg("example.com/bodyclose/settings", &dir.join("settings.go"));

    assert!(
        support::run_analyzer(bodyclose(), &pkg).is_empty(),
        "default should allow close-only: {:?}",
        support::run_analyzer(bodyclose(), &pkg)
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "bodyclose",
        BodycloseOptions {
            check_consumption: true,
        },
    );
    let messages = support::run_analyzer_with_settings(
        bodyclose(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("response body must be closed and consumed")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("closed and consumed"))
            .count()
            == 1,
        "only closedOnly should fail: {messages:?}"
    );
}
