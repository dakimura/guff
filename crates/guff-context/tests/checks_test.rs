mod support;

use std::sync::Arc;

use guff_analysis::SettingsBag;
use guff_context::{bodyclose, contextcheck, fatcontext, noctx, sqlclosecheck, BodycloseOptions};
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

#[test]
fn sqlclosecheck_flags_missing_and_non_defer() {
    let dir = support::testdata("sqlclosecheck");
    let pkg = support::typecheck_pkg("example.com/sqlclosecheck", &dir.join("bad.go"));
    let messages = support::run_analyzer(sqlclosecheck(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Rows/Stmt/NamedStmt was not closed")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("Close should use defer")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("was not closed"))
            .count()
            >= 3,
        "expected ≥3 not-closed (rows + stmt + reassign): {messages:?}"
    );
}

#[test]
fn sqlclosecheck_allows_defer_return_and_pass() {
    let dir = support::testdata("sqlclosecheck");
    let pkg = support::typecheck_pkg("example.com/sqlclosecheck/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(sqlclosecheck(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(sqlclosecheck(), &pkg)
    );
}

#[test]
fn contextcheck_flags_non_inherited_and_missing_ctx() {
    let dir = support::testdata("contextcheck");
    let pkg = support::typecheck_pkg("example.com/contextcheck", &dir.join("bad.go"));
    let messages = support::run_analyzer(contextcheck(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Non-inherited new context")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("should pass the context parameter")),
        "{messages:?}"
    );
}

#[test]
fn contextcheck_flags_closure_chain_like_helm() {
    // Mirrors helm's GetNewReplicaSet -> RsListFromClient -> $closure(Background)
    // pattern: a no-ctx helper builds a closure that passes context.Background
    // into a ctx-taking API; callers with ctx must be told to pass it down.
    let dir = support::testdata("contextcheck_closure");
    let pkg = support::typecheck_pkg(
        "example.com/contextcheck_closure",
        &dir.join("closure.go"),
    );
    let messages = support::run_analyzer(contextcheck(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("should pass the context parameter")),
        "{messages:?}"
    );
}

#[test]
fn contextcheck_ignores_noncapturing_closure_return() {
    // The same chain as the test above with the capture removed, which is the
    // whole difference: with no free variables go/ssa returns the bare
    // `*ssa.Function` and emits no `MakeClosure`, and upstream's `getCtxType`
    // answers only for calls and closures — a `return fn` is not followed, so
    // neither tool reports here. guff used to follow it, which cost a finding
    // nothing upstream produces on every non-capturing literal handed back
    // from a helper.
    let dir = support::testdata("contextcheck_nocapture");
    let pkg = support::typecheck_pkg(
        "example.com/contextcheck_nocapture",
        &dir.join("nocapture.go"),
    );
    let messages = support::run_analyzer(contextcheck(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn contextcheck_flags_capturing_closures_in_a_ctx_function() {
    // A func literal that captures anything becomes a `MakeClosure`, and
    // go/ssa emits that instruction with no position — so the only way a
    // diagnostic on it can land anywhere is the callee's own position.
    // Five literals here, one per way of reaching one: defer, go, immediate
    // call, assignment, and the non-capturing form that keeps its call's
    // position instead.
    let dir = support::testdata("contextcheck");
    let pkg = support::typecheck_pkg(
        "example.com/contextcheck_closures",
        &dir.join("closures.go"),
    );
    let messages = support::run_analyzer(contextcheck(), &pkg);
    assert_eq!(messages.len(), 5, "{messages:?}");
    for name in ["deferred", "spawned", "immediate", "assigned", "nocapture"] {
        let want = format!("Function `{name}$1->helper` should pass the context parameter");
        assert!(messages.iter().any(|m| *m == want), "{want}: {messages:?}");
    }
}

#[test]
fn contextcheck_accepts_http_request_context() {
    // `(*http.Request).Context` is a concrete method, so SSA emits a static
    // call with no interface method. Matching only on the invoke-mode method
    // made guff flag every canonical http.HandlerFunc body.
    let dir = support::testdata("contextcheck_httphandler");
    let pkg = support::typecheck_pkg("example.com/contextcheck/http/ok", &dir.join("ok.go"));
    let messages = support::run_analyzer(contextcheck(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn contextcheck_still_flags_background_in_http_handler() {
    let dir = support::testdata("contextcheck_httphandler");
    let pkg = support::typecheck_pkg("example.com/contextcheck/http/bad", &dir.join("bad.go"));
    let messages = support::run_analyzer(contextcheck(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("Non-inherited new context")),
        "context.Background() in a handler is still a finding: {messages:?}"
    );
}

#[test]
fn contextcheck_allows_inherited_context() {
    let dir = support::testdata("contextcheck");
    let pkg = support::typecheck_pkg("example.com/contextcheck/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(contextcheck(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(contextcheck(), &pkg)
    );
}
