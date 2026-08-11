mod support;

use guff_errcheck::{analyzer, analyzer_check_asserts, analyzer_check_blank};

#[test]
fn errcheck_flags_unchecked_error() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/errcheck/basic", &dir.join("bad.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("Error return value"));
    assert!(messages[0].contains("is not checked"));
}

#[test]
fn errcheck_flags_deferred_error() {
    let dir = support::testdata("defer");
    let pkg = support::typecheck_pkg("example.com/errcheck/defer", &dir.join("bad.go"));
    assert!(!pkg.ill_typed, "fixture must typecheck: {:?}", pkg.errors);
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("Error return value"), "{messages:?}");
}

#[test]
fn errcheck_allows_deferred_checked_error() {
    let dir = support::testdata("defer");
    let pkg = support::typecheck_pkg("example.com/errcheck/defer/ok", &dir.join("ok.go"));
    assert!(!pkg.ill_typed, "fixture must typecheck: {:?}", pkg.errors);
    assert!(support::run_analyzer(analyzer(), &pkg).is_empty());
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
    assert!(messages
        .iter()
        .all(|m| m.contains("Error return value") && m.contains("is not checked")));
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
    assert!(messages
        .iter()
        .all(|m| m.contains("Error return value") && m.contains("is not checked")));
}

#[test]
fn errcheck_assert_mode_allows_checked_assertions() {
    let dir = support::testdata("assert");
    let pkg = support::typecheck_pkg("example.com/errcheck/assert/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(analyzer_check_asserts(), &pkg).is_empty());
}

#[test]
fn errcheck_ignores_discarded_named_array_id() {
    // restic OSS hunt: discarded `restic.ID` must not be treated as error.
    let dir = support::testdata("named_id");
    let pkg = support::typecheck_pkg("example.com/errcheck/named_id", &dir.join("ok.go"));
    assert!(!pkg.ill_typed, "fixture must typecheck: {:?}", pkg.errors);
    assert!(
        support::run_analyzer(analyzer(), &pkg).is_empty(),
        "discarded ID return must not flag"
    );
}

#[test]
fn errcheck_exclude_functions_skips_listed_symbols() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_errcheck::Options;
    use guff_runner::RunnerOptions;

    let dir = support::testdata("exclude");
    let pkg = support::typecheck_pkg("example.com/errcheck/exclude", &dir.join("bad.go"));
    assert!(
        !pkg.ill_typed,
        "fixture must typecheck: {:?}",
        pkg.errors
    );

    let default_msgs = support::run_analyzer(analyzer(), &pkg);
    assert!(
        default_msgs.len() >= 2,
        "default should flag io.Copy and io.WriteString: {default_msgs:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "errcheck",
        Options {
            exclude_functions: vec!["io.Copy".into()],
            ..Options::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        analyzer(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(
        messages.len(),
        1,
        "exclude-functions: io.Copy should leave only WriteString: {messages:?}"
    );
}

#[test]
fn errcheck_disable_default_exclusions_flags_fmt_println() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_errcheck::Options;
    use guff_runner::RunnerOptions;

    let dir = support::testdata("default_exclude");
    let pkg = support::typecheck_pkg("example.com/errcheck/default_exclude", &dir.join("bad.go"));
    assert!(
        !pkg.ill_typed,
        "fixture must typecheck: {:?}",
        pkg.errors
    );

    assert!(
        support::run_analyzer(analyzer(), &pkg).is_empty(),
        "default exclusions should skip fmt.Println and hash.Hash.Write"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "errcheck",
        Options {
            disable_default_exclusions: true,
            ..Options::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        analyzer(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages.is_empty(),
        "disable-default-exclusions must flag fmt.Println: {messages:?}"
    );
}

#[test]
fn errcheck_flags_calls_to_instantiated_generics() {
    // `one[int]()` is an IndexExpr callee and `two[int, string](1)` an
    // IndexListExpr one. kisielk's `baseCallExpr` unwraps both, so upstream
    // reports them like any other call.
    let dir = support::testdata("generic");
    let pkg = support::typecheck_pkg("example.com/errcheck/generic", &dir.join("bad.go"));
    assert!(!pkg.ill_typed, "fixture must typecheck: {:?}", pkg.errors);
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
}

#[test]
fn errcheck_names_the_callee_the_way_golangci_does() {
    // Exact messages, not `contains("Error return value")`: the name between
    // the backticks is `cmp.Or(SelectorName, FuncName)`, and no gate compared
    // it before compat/golden/cases/errcheck (COMPAT-HARDENING §5 item 1).
    let dir = support::testdata("names");
    let pkg = support::typecheck_pkg("example.com/errcheck/names", &dir.join("bad.go"));
    assert!(!pkg.ill_typed, "fixture must typecheck: {:?}", pkg.errors);
    let mut messages = support::run_analyzer(analyzer(), &pkg);
    messages.sort();
    assert_eq!(
        messages,
        vec![
            "Error return value is not checked".to_string(),
            "Error return value is not checked".to_string(),
            "Error return value is not checked".to_string(),
            // The receiver is spelled without an import path here because the
            // test harness type-checks the file as a package with no path;
            // compat/golden/cases/errcheck has the qualified form.
            "Error return value of `(*writer).Flush` is not checked".to_string(),
            "Error return value of `(writer).Emit` is not checked".to_string(),
            "Error return value of `e.Emit` is not checked".to_string(),
            "Error return value of `pkgLevel.Emit` is not checked".to_string(),
            "Error return value of `w.Emit` is not checked".to_string(),
            "Error return value of `w.inner.Flush` is not checked".to_string(),
        ],
        "{messages:?}"
    );
}
