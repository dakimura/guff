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
fn ineffassign_skips_generated_files() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg(
        "example.com/ineffassign/generated",
        &dir.join("generated_ok.go"),
    );
    assert!(support::run_analyzer(analyzer(), &pkg).is_empty());
}
