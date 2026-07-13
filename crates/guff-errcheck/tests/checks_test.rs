mod support;

use guff_errcheck::{analyzer, analyzer_check_asserts, analyzer_check_blank};

#[test]
fn errcheck_flags_unchecked_error() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/errcheck/basic", &dir.join("bad.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("unchecked error"));
}

#[test]
fn errcheck_allows_checked_error() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/errcheck/basic/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(analyzer(), &pkg).is_empty());
}

#[test]
fn errcheck_blank_mode_flags_ignored_error_assignments() {
    let dir = support::testdata("blank");
    let pkg = support::typecheck_pkg("example.com/errcheck/blank", &dir.join("bad.go"));
    let messages = support::run_analyzer(analyzer_check_blank(), &pkg);
    assert!(messages.len() >= 4, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("unchecked error")));
}

#[test]
fn errcheck_blank_mode_allows_checked() {
    let dir = support::testdata("blank");
    let pkg = support::typecheck_pkg("example.com/errcheck/blank/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(analyzer_check_blank(), &pkg).is_empty());
}

#[test]
fn errcheck_assert_mode_flags_unchecked_type_assertions() {
    let dir = support::testdata("assert");
    let pkg = support::typecheck_pkg("example.com/errcheck/assert", &dir.join("bad.go"));
    let messages = support::run_analyzer(analyzer_check_asserts(), &pkg);
    assert!(messages.len() >= 4, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("unchecked error")));
}

#[test]
fn errcheck_assert_mode_allows_checked_assertions() {
    let dir = support::testdata("assert");
    let pkg = support::typecheck_pkg("example.com/errcheck/assert/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(analyzer_check_asserts(), &pkg).is_empty());
}
