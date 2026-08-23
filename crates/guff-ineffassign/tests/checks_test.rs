mod support;

use guff_ineffassign::analyzer;

#[test]
fn ineffassign_flags_if_branch_dead_store() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/ineffassign/if_bad", &dir.join("if_bad.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("ineffectual assignment"));
}

#[test]
fn ineffassign_flags_dead_assignment() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/ineffassign/basic", &dir.join("bad.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("ineffectual assignment"));
}

#[test]
fn ineffassign_allows_used_assignment() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/ineffassign/basic/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(analyzer(), &pkg).is_empty());
}

#[test]
fn ineffassign_allows_uses_inside_composite_literals() {
    // Locals used by-address / as element / as map key inside composite
    // literals are live; none of these assignments are ineffectual. Regression
    // for the missing CompositeLit/KeyValueExpr traversal in walk_expr that
    // caused false positives (e.g. prometheus config.go `retention`).
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg(
        "example.com/ineffassign/composite",
        &dir.join("composite_ok.go"),
    );
    assert!(
        support::run_analyzer(analyzer(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(analyzer(), &pkg)
    );
}

/// A composite-literal key that happens to spell a local variable.
///
/// gordonklaus/ineffassign has no `CompositeLit`/`KeyValueExpr` case, so its
/// walk reaches the key as a plain expression and resolves it through
/// `id.Obj` — go/parser's lexical scopes, which cannot tell `T{v: x}` (a field
/// name) from `map[K]V{v: x}` (a read of `v`) and so bind the key to the local.
/// The assignment is therefore *used* upstream. Following go/types instead
/// reports it, which is what guff did on grafana's
/// `pkg/storage/unified/resource/storage_backend.go:247` — a false positive
/// that only became visible once that package stopped being ill-typed.
#[test]
fn ineffassign_treats_a_field_key_spelled_like_a_local_as_a_use() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg(
        "example.com/ineffassign/composite",
        &dir.join("composite_ok.go"),
    );
    assert!(
        support::run_analyzer(analyzer(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(analyzer(), &pkg)
    );
}

/// The other side of it: a key that does not spell the local leaves the
/// assignment dead, so guff must still report it. Without this, "resolve the
/// key in scope" could degrade into "never report an assignment made near a
/// composite literal" and no test would notice.
#[test]
fn ineffassign_still_flags_a_dead_store_when_the_key_names_another_field() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg(
        "example.com/ineffassign/compositekey",
        &dir.join("composite_key_bad.go"),
    );
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("ineffectual assignment to lookback"));
}

#[test]
fn ineffassign_flags_switch_fallthrough_dead_store() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/ineffassign/switch", &dir.join("switch_bad.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("ineffectual assignment"));
}

#[test]
fn ineffassign_flags_named_return_dead_store() {
    let dir = support::testdata("basic");
    let pkg =
        support::typecheck_pkg("example.com/ineffassign/named", &dir.join("named_return_bad.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("ineffectual assignment"));
}

#[test]
fn ineffassign_allows_naked_named_return() {
    // `ls = …; return` with named result `ls` is not ineffectual.
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg(
        "example.com/ineffassign/named_ok",
        &dir.join("named_return_ok.go"),
    );
    assert!(
        support::run_analyzer(analyzer(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(analyzer(), &pkg)
    );
}

#[test]
fn ineffassign_allows_named_result_assign_in_defer() {
    // Assignment to a named result from a deferred closure must not be flagged
    // (closure capture escapes the outer var).
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg(
        "example.com/ineffassign/defer_named",
        &dir.join("defer_named_ok.go"),
    );
    assert!(
        support::run_analyzer(analyzer(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(analyzer(), &pkg)
    );
}

#[test]
fn ineffassign_allows_assignment_used_after_goto() {
    // `pos = …; goto chomp` where chomp uses `pos` must not be flagged.
    // Regression for containerd pkg/filters/scanner.go.
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/ineffassign/goto", &dir.join("goto_ok.go"));
    assert!(
        support::run_analyzer(analyzer(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(analyzer(), &pkg)
    );
}

#[test]
fn ineffassign_skips_generated_files() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg(
        "example.com/ineffassign/generated",
        &dir.join("generated_ok.go"),
    );
    assert!(support::run_analyzer(analyzer(), &pkg).is_empty());
}

#[test]
fn ineffassign_allows_zero_init_define_overwritten() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg(
        "example.com/ineffassign/zeroinit",
        &dir.join("zero_init_ok.go"),
    );
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert!(
        messages.is_empty(),
        "expected no findings for zero-init := overwritten before use, got {messages:?}"
    );
}
