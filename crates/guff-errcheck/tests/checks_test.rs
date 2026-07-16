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
        "default exclusions should skip fmt.Println"
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
