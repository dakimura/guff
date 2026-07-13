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
fn ineffassign_skips_generated_files() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg(
        "example.com/ineffassign/generated",
        &dir.join("generated_ok.go"),
    );
    assert!(support::run_analyzer(analyzer(), &pkg).is_empty());
}
