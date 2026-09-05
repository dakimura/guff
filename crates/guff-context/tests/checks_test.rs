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
    assert_eq!(messages, vec!["nested context in loop"], "{messages:?}");
}

#[test]
fn fatcontext_asks_whether_the_variable_was_declared_inside_the_node() {
    // `isWithinLoop` — the badly named predicate behind every category — is
    // asked with the node the report would be attributed to, and for an
    // assignment written in a plain function body that node is the `FuncDecl`.
    // A `context.Context` parameter's scope *is* the function body, so it lies
    // inside its own declaration and upstream leaves it alone. guff's span
    // helper had arms for `ForStmt`, `RangeStmt` and `FuncLit` and none for
    // `FuncDecl` — the node had been added to the body lookup for the
    // struct-pointer category and not here — so the predicate answered "not
    // within" for every one of them (prometheus, 14 findings).
    //
    // Twelve shapes: the eight that must stay silent are the assertion, and the
    // three that fire are what proves the predicate is still being asked.
    let dir = support::testdata("fatcontext");
    let pkg = support::typecheck_pkg("example.com/fatcontext/scope", &dir.join("scope.go"));
    let messages = support::run_analyzer(fatcontext(), &pkg);
    assert_eq!(
        messages,
        vec![
            // packageVarAtTopLevel
            "nested context in function literal",
            // paramInLoop, packageVarInLoop
            "nested context in loop",
            "nested context in loop",
        ],
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

/// Every reporting shape in `testdata/bodyclose/bad.go` — the same keys
/// `compat/golden/cases/bodyclose` pins against golangci-lint. Raise it when
/// the fixture grows; a drop is a shape that stopped reporting.
///
/// Eleven of the twenty-one are the merge shapes: two stores to one variable
/// are two SSA values, and the second kills the first only when it dominates
/// it. The ones that merge instead reach a `*ssa.Phi` and a single close
/// settles them all — those live in `ok.go`.
const BODYCLOSE_BAD_SHAPES: usize = 21;

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
fn bodyclose_follows_the_referrers_of_the_response() {
    // Upstream decides with `isopen`, a walk over the response value's SSA
    // referrers. Six of its branches had no counterpart in guff's AST port:
    //
    //   * `getReqCall` accepts *any* call whose result mentions
    //     `*http.Response`, so a helper of this package is a candidate too, and
    //     a result nobody binds has no referrers — reported. guff only ever
    //     tracked assignments (connect-go, four times).
    //   * a store into a global is skipped outright ("referrers for globals are
    //     always nil"), and guff reported it.
    //   * a store into a field is settled by a close reached through that field.
    //   * a capture by a func literal is settled however the literal uses the
    //     body — `defer func() { io.Copy(io.Discard, resp.Body) }()` included,
    //     which guff read as a leak (connect-go's `bench_test.go`).
    //   * handing the response to a callee settles it only when that callee
    //     closes; guff treated every argument position as handing over
    //     ownership, which silenced `d.validateResponse(response)`.
    //
    // The six silent shapes are the assertion here; the four that fire are what
    // keeps the walk from simply going quiet.
    let dir = support::testdata("bodyclose");
    let pkg = support::typecheck_pkg("example.com/bodyclose/referrers", &dir.join("referrers.go"));
    let messages = support::run_analyzer(bodyclose(), &pkg);
    assert_eq!(messages.len(), 4, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m == "response body must be closed"),
        "{messages:?}"
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

/// Reads the **isolate** fixture, which is also the golden case's source, so
/// the two gates cannot measure different files. The crate had its own
/// forty-six-line copy of `bad.go` that no compat tier ever ran, and the only
/// assertion on it was `>= 3` — a floor no defect can fail.
#[test]
fn sqlclosecheck_flags_missing_and_non_defer() {
    let pkg = support::typecheck_with_stubs_from(
        "example.com/sqlclosecheck",
        &support::isolate_fixture("sqlclosecheck", "bad.go"),
        &support::testdata("sqlclosecheck"),
    );
    let messages = support::run_analyzer(sqlclosecheck(), &pkg);
    let not_closed = messages
        .iter()
        .filter(|m| m.contains("Rows/Stmt/NamedStmt was not closed"))
        .count();
    let use_defer = messages
        .iter()
        .filter(|m| m.contains("Close should use defer"))
        .count();
    // Counted. Four of the not-closed are store destinations that are *not*
    // an `*ssa.FieldAddr` — a slice element, a map entry, a pointer
    // indirection, and a plain copy to another local, which is reported once
    // and not twice because only an `*ssa.Call` starts a value. The four
    // `FieldAddr` shapes beside them draw nothing.
    assert_eq!((not_closed, use_defer), (SQLCLOSE_NOT_CLOSED, 1), "{messages:?}");
}

/// Every "was not closed" shape in `compat/isolate/fixtures/sqlclosecheck/bad.go`
/// — the same keys `compat/golden/cases/sqlclosecheck` pins against
/// golangci-lint. A drop is a shape that stopped reporting.
const SQLCLOSE_NOT_CLOSED: usize = 6;

#[test]
fn sqlclosecheck_settles_a_phi_and_a_close_inside_a_returned_literal() {
    // Upstream decides on the SSA value: two assignments in the arms of an
    // `if` meet in one φ, and one close settles every edge into it. This port
    // tracks a name in a statement list, so the second assignment orphaned the
    // first and reported it — syncthing's `PrefixKV`, twice over. A
    // *sequential* reassignment is not that shape: the first really does lose
    // its rows, and upstream reports it even though a close follows the second.
    //
    // The close itself only counted when it sat in a `defer func(){ … }()` at
    // the site; `PrefixKV` hands the closure back instead. A capture on its own
    // settles nothing here, which is where this differs from bodyclose.
    let pkg = support::typecheck_with_stubs_from(
        "example.com/sqlclosecheck/branches",
        &support::isolate_fixture("sqlclosecheck", "branches.go"),
        &support::testdata("sqlclosecheck"),
    );
    let messages = support::run_analyzer(sqlclosecheck(), &pkg);
    // Two never-closed arms, the first of the sequential pair, and the capture
    // that never closes. `BranchesClosedAfter` and `ClosedInReturnedClosure`
    // are the two that must stay silent.
    assert_eq!(messages.len(), 4, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m == "Rows/Stmt/NamedStmt was not closed"),
        "{messages:?}"
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

/// A closure is filed under its parent's name, not under `"run$1"`.
///
/// `(*ssa.Function).RelString(nil)` — the key for both the entry memo and the
/// exported fact — qualifies an anonymous function by its parent:
///
/// ```text
/// if f.parent != nil {
///     parent := f.parent.RelString(from)
///     for i, anon := range f.parent.AnonFuncs {
///         if anon == f { return fmt.Sprintf("%s$%d", parent, 1+i) }
///     }
///     return f.name // should never happen
/// }
/// ```
///
/// guff returned the bare `Function.name`, so **every `run` method in a package
/// shared one key**. The fixture has three, and only the first captures a
/// context: with a shared key the other two inherited `EntryWithCtx` and guff
/// reported all three. k6's `internal/cmd` has five `run` methods, and that is
/// how `(*cmdCloudRun).run`'s closure came to report a chain upstream never
/// reaches (`cloud_run.go:151`).
#[test]
fn contextcheck_keys_a_closure_by_its_parent() {
    let dir = support::testdata("contextcheck_anonkey");
    let pkg = support::typecheck_pkg(
        "example.com/contextcheck_anonkey",
        &dir.join("anonkey.go"),
    );
    let messages = support::run_analyzer(contextcheck(), &pkg);
    // One: the closure of the method that has a context to pass down.
    assert_eq!(
        messages,
        vec!["Function `chainTop->chainMiddle->chainBottom` should pass the context parameter"],
        "{messages:?}"
    );
}
