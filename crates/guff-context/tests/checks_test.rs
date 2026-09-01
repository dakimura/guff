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
    // An exact count, not `>= 5`: a floor passes while every shape but one
    // stops reporting, which is how the `//nolint:unparam` sibling defect in
    // unparam survived a green suite.
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("response body must be closed"))
            .count(),
        BODYCLOSE_BAD_SHAPES,
        "{messages:?}"
    );
}

/// Every reporting shape in `testdata/bodyclose/bad.go` — the same ten keys
/// `compat/golden/cases/bodyclose` pins against golangci-lint. Raise it when
/// the fixture grows; a drop is a shape that stopped reporting.
const BODYCLOSE_BAD_SHAPES: usize = 10;

#[test]
fn bodyclose_skips_packages_without_a_direct_net_http_import() {
    // Upstream's first act is
    // `analysisutil.LookupFromImports(pass.Pkg.Imports(), "net/http", "Response")`,
    // and `Imports()` is the package's *direct* imports. A package that only
    // reaches `*http.Response` through a dependency is not checked at all —
    // scaleway-cli's `internal/gotty`, which dials with `gorilla/websocket`.
    let dir = support::testdata("bodyclose");
    let pkg = support::typecheck_pkg("example.com/bodyclose/nohttp", &dir.join("nohttp.go"));
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);
    let messages = support::run_analyzer(bodyclose(), &pkg);
    assert!(
        messages.is_empty(),
        "no direct net/http import: upstream checks nothing here: {messages:?}"
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
    // `badAssign` builds a context and never passes it anywhere, which is not a
    // finding — counting instead of `any(contains(..))` is what keeps that
    // third function measured.
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert_eq!(
        messages.iter().filter(|m| m.contains("Non-inherited new context")).count(),
        1,
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| *m == "Function `inner` should pass the context parameter"),
        "{messages:?}"
    );
}

#[test]
fn contextcheck_stops_at_a_replaced_context() {
    // Upstream's `collectCtxRef` returns `ok = false` the moment it reports a
    // non-inherited context, and `checkFuncWithCtx` then returns: a function
    // that threw its own context away is asked nothing more, so the callee
    // chains it would otherwise be told about stay silent. guff kept going and
    // invented them (scaleway-cli `core/bootstrap.go:252`, where line 205 is
    // `ctx = context.Background() //nolint: contextcheck`).
    //
    // Seven shapes, and which of the two diagnostics each one produces is the
    // point: a *store* or a *phi* silences the chain, a bare call on a
    // replaced register does not.
    let dir = support::testdata("contextcheck_reassign");
    let pkg = support::typecheck_pkg(
        "example.com/contextcheck_reassign",
        &dir.join("reassign.go"),
    );
    let messages = support::run_analyzer(contextcheck(), &pkg);
    assert_eq!(messages.len(), 7, "{messages:?}");
    let chain = "Function `helper` should pass the context parameter";
    let fresh = "Non-inherited new context, use function like `context.WithXXX` instead";
    // NoReassign, ReassignPlain, ReassignInherited.
    assert_eq!(messages.iter().filter(|m| *m == chain).count(), 3, "{messages:?}");
    // ReassignPlain's own call, then Phi / Captured / Loop.
    assert_eq!(messages.iter().filter(|m| *m == fresh).count(), 4, "{messages:?}");
}

#[test]
fn contextcheck_skips_bound_method_values() {
    // `install(c.Complete)` builds go/ssa's `$bound` closure target, a function
    // distinct from the method, and `RelString` keeps the suffix. contextcheck
    // leans on that: the suffixed key has no verdict, so a method handed to a
    // library never drags its own chain into the caller. guff built the fact
    // key from the method *object* alone, dropped the suffix, and reported.
    // The method expression (`$thunk`) is the same story; only the direct call
    // is a finding.
    let dir = support::testdata("contextcheck_boundmethod");
    let pkg = support::typecheck_pkg(
        "example.com/contextcheck_boundmethod",
        &dir.join("bound.go"),
    );
    let messages = support::run_analyzer(contextcheck(), &pkg);
    assert_eq!(
        messages,
        vec!["Function `Complete->autoComplete` should pass the context parameter"],
        "{messages:?}"
    );
}

#[test]
fn contextcheck_reads_its_own_nolint_doc_directive() {
    // This is upstream's `docFlag`, not golangci-lint's `//nolint` processor:
    // a skipped function records no verdict at all, so its *callers* fall
    // silent too — somewhere the processor would never have reached. Five
    // shapes pin the regexp `^//\s?nolint:` and the `contextcheck` substring:
    // no space and one space skip, another linter's directive, a space before
    // the colon, and a plain doc comment do not.
    let dir = support::testdata("contextcheck_docflag");
    let pkg = support::typecheck_pkg(
        "example.com/contextcheck_docflag",
        &dir.join("docflag.go"),
    );
    let messages = support::run_analyzer(contextcheck(), &pkg);
    assert_eq!(messages.len(), 3, "{messages:?}");
    for name in ["notSkippedOtherLinter", "notSkippedSpacedColon", "plainDoc"] {
        let want = format!("Function `{name}` should pass the context parameter");
        assert!(messages.iter().any(|m| *m == want), "{want}: {messages:?}");
    }
}

#[test]
fn contextcheck_req_has_ctx_directive_makes_an_entry() {
    // `// @contextcheck(req_has_ctx)` promotes any function taking an
    // `*http.Request` to a handler, without the canonical two-parameter shape.
    // Without it the same function is an ordinary no-context callee, and the
    // finding moves to its caller.
    let dir = support::testdata("contextcheck_reqctx");
    let pkg =
        support::typecheck_pkg("example.com/contextcheck_reqctx", &dir.join("reqctx.go"));
    let messages = support::run_analyzer(contextcheck(), &pkg);
    assert_eq!(messages.len(), 3, "{messages:?}");
    let handler = "Non-inherited new context, use function like `context.WithXXX` or `r.Context` instead";
    // TaggedHandler (directive) and CanonicalHandler (shape).
    assert_eq!(messages.iter().filter(|m| *m == handler).count(), 2, "{messages:?}");
    assert!(
        messages
            .iter()
            .any(|m| m == "Function `PlainRequest` should pass the context parameter"),
        "{messages:?}"
    );
}

#[test]
fn contextcheck_ignores_a_fresh_context_that_goes_nowhere() {
    // `context.Background()` returns a context, so upstream classifies the call
    // as ctx-*out* and skips it outright — building one and dropping it is not
    // a finding. guff carried an extra guard that matched the callee by name
    // and condemned the function; only the closure that actually captures the
    // context is reported.
    let dir = support::testdata("contextcheck_background");
    let pkg = support::typecheck_pkg(
        "example.com/contextcheck_background",
        &dir.join("background.go"),
    );
    let messages = support::run_analyzer(contextcheck(), &pkg);
    assert_eq!(
        messages,
        vec!["Function `bgToClosure` should pass the context parameter"],
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
