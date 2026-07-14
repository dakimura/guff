mod support;

use guff_context::{fatcontext, noctx};

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
