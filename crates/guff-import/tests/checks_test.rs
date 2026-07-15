mod support;

use guff_import::{
    analyzer_block_logrus, analyzer_local_replace, depguard, gomoddirectives, gomodguard,
};

#[test]
fn depguard_flags_non_stdlib_imports() {
    let pkg = support::typecheck_fixture("depguard", "example.com/depguard", "bad.go");
    let messages = support::run_analyzer(depguard(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("github.com/foo/bar") && m.contains("not allowed")),
        "{messages:?}"
    );
}

#[test]
fn depguard_allows_stdlib() {
    let pkg = support::typecheck_fixture("depguard", "example.com/depguard/ok", "ok.go");
    assert!(support::run_analyzer(depguard(), &pkg).is_empty());
}

#[test]
fn gomoddirectives_flags_replace() {
    let pkg = support::typecheck_fixture(
        "gomoddirectives/bad",
        "example.com/gomoddirectives/bad",
        "main.go",
    );
    let messages = support::run_analyzer(gomoddirectives(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("replacement") || m.contains("local replacement")),
        "{messages:?}"
    );
}

#[test]
fn gomoddirectives_allows_clean_gomod() {
    let pkg = support::typecheck_fixture(
        "gomoddirectives/ok",
        "example.com/gomoddirectives/ok",
        "main.go",
    );
    assert!(support::run_analyzer(gomoddirectives(), &pkg).is_empty());
}

#[test]
fn gomodguard_default_is_quiet() {
    let pkg = support::typecheck_fixture("gomodguard/ok", "example.com/gomodguard/ok", "main.go");
    assert!(support::run_analyzer(gomodguard(), &pkg).is_empty());
}

#[test]
fn gomodguard_flags_blocked_module_import() {
    let pkg = support::typecheck_fixture(
        "gomodguard/blocked",
        "example.com/gomodguard/blocked",
        "main.go",
    );
    let messages = support::run_analyzer(analyzer_block_logrus(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("logrus") && m.contains("blocked")),
        "{messages:?}"
    );
}

#[test]
fn gomodguard_flags_local_replace_import() {
    let pkg = support::typecheck_fixture(
        "gomodguard/localreplace",
        "example.com/gomodguard/localreplace",
        "main.go",
    );
    let messages = support::run_analyzer(analyzer_local_replace(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("local replace") && m.contains("github.com/foo/bar")),
        "{messages:?}"
    );
}
