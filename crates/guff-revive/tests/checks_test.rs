mod support;

use guff_analysis::validate;
use guff_revive::revive;

#[test]
fn revive_flags_default_rule_violations() {
    let pkg = support::typecheck_fixture("revive", "example.com/revive", "bad.go");
    let messages = support::run_analyzer(revive(), &pkg);
    for needle in [
        "dot-imports:",
        "blank-imports:",
        "increment-decrement:",
        "error-naming:",
        "error-strings:",
        "redefines-builtin-id:",
        "receiver-naming:",
        "range:",
        "errorf:",
        "error-return:",
        "var-declaration:",
        "package-comments:",
        "var-naming:",
        "unexported-return:",
        "unused-parameter:",
        "unreachable-code:",
        "indent-error-flow:",
        "superfluous-else:",
    ] {
        assert!(
            messages.iter().any(|m| m.contains(needle)),
            "missing {needle} in {messages:?}"
        );
    }
}

#[test]
fn revive_allows_clean_code() {
    let pkg = support::typecheck_fixture("revive", "example.com/revive/ok", "ok.go");
    let messages = support::run_analyzer(revive(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn revive_analyzer_graph_is_valid() {
    validate(&[revive()]).expect("valid analyzer graph");
}

#[test]
fn revive_flags_extended_rule_violations() {
    guff_revive::with_extended_rules(|| {
        let pkg = support::typecheck_fixture(
            "revive",
            "example.com/revive/extended",
            "extended_bad.go",
        );
        let messages = support::run_analyzer(revive(), &pkg);
        for needle in [
            "atomic:",
            "bare-return:",
            "bool-literal-in-expr:",
            "call-to-gc:",
            "cyclomatic:",
            "duplicated-imports:",
            "use-errors-new:",
            "waitgroup-by-value:",
            "string-of-int:",
            "time-equal:",
            "unchecked-type-assertion:",
            "unconditional-recursion:",
            "if-return:",
            "unnecessary-format:",
            "cognitive-complexity:",
            "constant-logical-expr:",
            "import-shadowing:",
            "struct-tag:",
            "time-date:",
            "unhandled-error:",
            "unnecessary-stmt:",
            "add-constant:",
            "argument-limit:",
            "early-return:",
            "deep-exit:",
            "get-return:",
            "redundant-import-alias:",
            "unnecessary-if:",
            "defer:",
            "flag-parameter:",
            "function-length:",
            "function-result-limit:",
            "use-any:",
            "use-fmt-print:",
            "unused-receiver:",
            "modifies-parameter:",
            "identical-branches:",
            "identical-ifelseif-branches:",
            "identical-ifelseif-conditions:",
            "identical-switch-branches:",
            "identical-switch-conditions:",
            "line-length-limit:",
            "max-control-nesting:",
            "nested-structs:",
            "unexported-naming:",
            "empty-lines:",
            "optimize-operands-order:",
            "range-val-in-closure:",
            "confusing-results:",
            "confusing-naming:",
            "imports-blocklist:",
            "string-format:",
            "file-header:",
            "import-alias-naming:",
            "useless-break:",
            "useless-fallthrough:",
            "modifies-value-receiver:",
            "range-val-address:",
            "unsecure-url-scheme:",
            "banned-characters:",
            "file-length-limit:",
            "multiline-if-init:",
            "package-naming:",
            "use-slices-sort:",
            "inefficient-map-lookup:",
            "comment-spacings:",
            "epoch-naming:",
            "comments-density:",
            "datarace:",
            "enforce-map-style:",
            "enforce-slice-style:",
            "enforce-switch-style:",
            "forbidden-call-in-wg-go:",
        ] {
            assert!(
                messages.iter().any(|m| m.contains(needle)),
                "missing {needle} in {messages:?}"
            );
        }
    });
}

#[test]
fn revive_flags_filename_format_violation() {
    guff_revive::with_extended_rules(|| {
        let pkg = support::typecheck_fixture(
            "revive",
            "example.com/revive/badfile",
            "bad file.go",
        );
        let messages = support::run_analyzer(revive(), &pkg);
        assert!(
            messages.iter().any(|m| m.contains("filename-format:")),
            "missing filename-format in {messages:?}"
        );
    });
}

#[test]
fn revive_flags_redundant_test_main_exit() {
    guff_revive::with_extended_rules(|| {
        let pkg = support::typecheck_fixture(
            "revive",
            "example.com/revive/util_test",
            "extended_bad_test.go",
        );
        let messages = support::run_analyzer(revive(), &pkg);
        assert!(
            messages.iter().any(|m| m.contains("redundant-test-main-exit:")),
            "missing redundant-test-main-exit in {messages:?}"
        );
    });
}

#[test]
fn revive_flags_enforce_repeated_arg_type_style() {
    let mut settings = guff_revive::extended_test_settings();
    if let Some(rules) = settings.rules.as_mut() {
        if let Some(rule) = rules
            .iter_mut()
            .find(|r| r.name == "enforce-repeated-arg-type-style")
        {
            rule.arguments = vec![guff_revive::RuleArgument::String("short".into())];
        }
    }
    guff_revive::with_settings(settings, || {
        let pkg = support::typecheck_fixture(
            "revive",
            "example.com/revive/extended",
            "extended_bad.go",
        );
        let messages = support::run_analyzer(guff_revive::revive(), &pkg);
        assert!(
            messages
                .iter()
                .any(|m| m.contains("enforce-repeated-arg-type-style:")),
            "missing enforce-repeated-arg-type-style in {messages:?}"
        );
    });
}

#[test]
fn revive_flags_package_directory_mismatch() {
    let mut settings = guff_revive::extended_test_settings();
    if let Some(rules) = settings.rules.as_mut() {
        if let Some(rule) = rules.iter_mut().find(|r| r.name == "package-directory-mismatch") {
            rule.arguments = vec![guff_revive::RuleArgument::Map({
                let mut map = std::collections::HashMap::new();
                map.insert(
                    "ignore-directories".into(),
                    guff_revive::RuleArgument::List(Vec::new()),
                );
                map
            })];
        }
    }
    guff_revive::with_settings(settings, || {
        let pkg = support::typecheck_fixture(
            "revive",
            "example.com/revive/extended",
            "extended_bad.go",
        );
        let messages = support::run_analyzer(guff_revive::revive(), &pkg);
        assert!(
            messages
                .iter()
                .any(|m| m.contains("package-directory-mismatch:")),
            "missing package-directory-mismatch in {messages:?}"
        );
    });
}

#[test]
fn revive_extended_allows_clean_code() {
    guff_revive::with_extended_rules(|| {
        let pkg = support::typecheck_fixture(
            "revive",
            "example.com/revive/extended/ok",
            "extended_ok.go",
        );
        let messages = support::run_analyzer(revive(), &pkg);
        assert!(messages.is_empty(), "{messages:?}");
    });
}

#[test]
fn revive_applies_per_rule_and_global_severity() {
    use guff_analysis::SettingsBag;
    use guff_revive::{RuleSetting, Settings};
    use guff_runner::{run_on_packages, RunnerOptions};
    use std::sync::Arc;

    let settings = Settings {
        severity: Some("warning".into()),
        rules: Some(vec![
            RuleSetting {
                name: "dot-imports".into(),
                arguments: Vec::new(),
                disabled: false,
                severity: Some("error".into()),
            },
            RuleSetting {
                name: "blank-imports".into(),
                arguments: Vec::new(),
                disabled: false,
                severity: None,
            },
        ]),
    };
    let mut bag = SettingsBag::new();
    bag.insert("revive", settings);
    let bag = Arc::new(bag);

    let pkg = support::typecheck_fixture("revive", "example.com/revive", "bad.go");
    let result = run_on_packages(
        &[revive()],
        std::slice::from_ref(&pkg),
        &RunnerOptions {
            sequential: true,
            settings: bag,
            ..RunnerOptions::default()
        },
    )
    .expect("run revive");

    let severities: Vec<String> = result
        .diagnostics()
        .into_iter()
        .map(|(_, d)| d.severity.clone())
        .collect();
    assert!(
        severities.iter().any(|s| s == "error"),
        "dot-imports should be error: {severities:?}"
    );
    assert!(
        severities.iter().any(|s| s == "warning"),
        "blank-imports should inherit global warning: {severities:?}"
    );
}
