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
        "empty-block:",
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
fn revive_blank_imports_allows_justified_contiguous_group() {
    let pkg = support::typecheck_fixture(
        "revive",
        "example.com/revive/blank_group_ok",
        "blank_group_ok.go",
    );
    let messages = support::run_analyzer(revive(), &pkg);
    assert!(
        messages.iter().all(|m| !m.contains("blank-imports")),
        "justified blank-import group must be silent: {messages:?}"
    );
}

#[test]
fn revive_blank_imports_flags_only_first_of_unjustified_group() {
    let pkg = support::typecheck_fixture(
        "revive",
        "example.com/revive/blank_group_bad",
        "blank_group_bad.go",
    );
    let messages: Vec<_> = support::run_analyzer(revive(), &pkg)
        .into_iter()
        .filter(|m| m.contains("blank-imports"))
        .collect();
    assert_eq!(
        messages.len(),
        1,
        "contiguous unjustified blank imports report once: {messages:?}"
    );
}

#[test]
fn revive_package_comments_accepts_sibling_file_doc() {
    let pkg = support::typecheck_fixture_dir(
        "revive",
        "sibling_ok",
        "example.com/revive/sibling_ok",
    );
    let messages = support::run_analyzer(revive(), &pkg);
    assert!(
        messages.iter().all(|m| !m.contains("package-comments")),
        "package comment on sibling file must silence: {messages:?}"
    );
}

#[test]
fn revive_exported_skips_methods_on_private_receivers() {
    let pkg = support::typecheck_fixture(
        "revive",
        "example.com/revive/private_receiver_ok",
        "private_receiver_ok.go",
    );
    let messages = support::run_analyzer(revive(), &pkg);
    assert!(
        messages.iter().all(|m| !m.contains("exported:")),
        "private receivers / common methods must be silent: {messages:?}"
    );
}

#[test]
fn revive_var_declaration_does_not_descend_into_values() {
    let pkg = support::typecheck_fixture(
        "revive",
        "example.com/revive/vardeclprune",
        "var_decl_prune.go",
    );
    let messages = support::run_analyzer(revive(), &pkg);
    let decls: Vec<&String> = messages
        .iter()
        .filter(|m| m.contains("var-declaration:"))
        .collect();
    assert!(
        decls.iter().any(|m| m.contains("var TopLevel")),
        "a plain declaration is still reported: {decls:?}"
    );
    assert!(
        decls
            .iter()
            .all(|m| !m.contains("inClosure") && !m.contains("inBlank")),
        "vars inside a declaration's value are invisible upstream: {decls:?}"
    );
}

#[test]
fn revive_exported_names_generic_receivers_like_upstream() {
    let pkg = support::typecheck_fixture(
        "revive",
        "example.com/revive/genericreceiver",
        "generic_receiver.go",
    );
    let messages = support::run_analyzer(revive(), &pkg);
    let exported: Vec<&String> = messages
        .iter()
        .filter(|m| m.contains("exported:"))
        .collect();
    assert!(
        exported
            .iter()
            .any(|m| m.contains("exported method Box.Get should have comment")),
        "pointer receiver on a generic type must read `Box.Get`: {exported:?}"
    );
    assert!(
        exported
            .iter()
            .any(|m| m.contains("exported method Pair.First should have comment")),
        "two-parameter receiver must read `Pair.First`: {exported:?}"
    );
    assert!(
        exported.iter().all(|m| !m.contains("Exported")),
        "methods on the unexported generic type must stay silent: {exported:?}"
    );
    assert!(
        exported.iter().all(|m| !m.contains("IndexExpr") && !m.contains('*')),
        "receiver must be the bare type name: {exported:?}"
    );
}

#[test]
fn revive_exported_skips_sort_interface_methods() {
    let pkg = support::typecheck_fixture(
        "revive",
        "example.com/revive/sortable_ok",
        "sortable_ok.go",
    );
    let messages = support::run_analyzer(revive(), &pkg);
    assert!(
        messages.iter().all(|m| !m.contains("exported:")),
        "sort.Interface Len/Less/Swap must be silent: {messages:?}"
    );
}

#[test]
fn revive_analyzer_graph_is_valid() {
    validate(&[revive()]).expect("valid analyzer graph");
}

#[test]
fn revive_duplicated_imports_reports_the_import_spec_not_the_path() {
    // Upstream passes `Node: imp` — the whole ImportSpec — so the column is the
    // alias's when there is one. Only the *path* is compared for duplication,
    // which is why `import "os"` and `import osdup "os"` are a duplicate pair
    // at all: Go accepts them, and revive still reports the second one.
    //
    // extended_bad.go:14 is the bare `import "os"` and :15 the aliased one; on
    // an unaliased spec ImportSpec.Pos() *is* the path's position, so a rule
    // that reports the path looks correct until an alias appears.
    guff_revive::with_extended_rules(|| {
        let pkg = support::typecheck_fixture(
            "revive",
            "example.com/revive/extended",
            "extended_bad.go",
        );
        let found: Vec<String> = support::run_analyzer_at(revive(), &pkg)
            .into_iter()
            .filter(|m| m.contains("duplicated-imports:"))
            .collect();
        assert_eq!(
            found,
            vec!["15:8: duplicated-imports: Package \"os\" already imported".to_string()],
            "duplicated-imports must report the ImportSpec's column (8, the alias), \
             not the path's (14)"
        );
    });
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
            // range-val-in-closure and range-val-address are absent on purpose:
            // upstream returns early for packages on Go 1.22+, where each
            // iteration already gets its own copy of the loop variable, so
            // neither capturing it nor taking its address is a bug any more.
            // guff does the same. The fixture has no module, which reads as
            // "new enough".
            "confusing-results:",
            "confusing-naming:",
            "imports-blocklist:",
            "string-format:",
            "file-header:",
            "import-alias-naming:",
            "useless-break:",
            "useless-fallthrough:",
            "modifies-value-receiver:",
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
fn revive_flags_function_length() {
    // Not part of the extended_bad.go sweep: upstream's function-length bails
    // out of a whole file once it meets an empty-bodied function, and
    // extended_bad.go has one near the top. This fixture has none.
    guff_revive::with_extended_rules(|| {
        let pkg = support::typecheck_fixture(
            "revive",
            "example.com/revive/funclen",
            "function_length_bad.go",
        );
        let messages = support::run_analyzer(revive(), &pkg);
        assert!(
            messages
                .iter()
                .any(|m| m.contains("function-length: maximum number of statements")),
            "missing function-length in {messages:?}"
        );
    });
}

#[test]
fn revive_function_length_is_silenced_by_an_empty_body() {
    // The upstream quirk itself: one `func f() {}` above the long function and
    // the rule reports nothing for the file.
    guff_revive::with_extended_rules(|| {
        let pkg = support::typecheck_fixture(
            "revive",
            "example.com/revive/extended",
            "extended_bad.go",
        );
        let messages = support::run_analyzer(revive(), &pkg);
        assert!(
            messages.iter().all(|m| !m.contains("function-length:")),
            "function-length must be silent after an empty body: {messages:?}"
        );
    });
}

#[test]
fn revive_flags_forbidden_call_in_wg_go() {
    // The same rule extended_bad.go covers, in a file of its own so that the
    // golden tier can materialize it into a `go 1.25` module — the rule is
    // gated on the *package's* Go version, which no build tag can raise.
    guff_revive::with_extended_rules(|| {
        let pkg = support::typecheck_fixture(
            "revive",
            "example.com/revive/wggo",
            "wg_go_bad.go",
        );
        let messages = support::run_analyzer(revive(), &pkg);
        for needle in [
            "forbidden-call-in-wg-go: do not call wg.Done inside wg.Go",
            "forbidden-call-in-wg-go: do not call panic inside wg.Go",
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
        confidence: None,
        ignore_generated_header: false,
        enable_default_rules: false,
        enable_all_rules: false,
        go: None,
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

#[test]
fn revive_filters_failures_below_confidence_threshold() {
    use guff_revive::{with_settings, Settings};

    with_settings(Settings {
        confidence: Some(0.9),
        ..Settings::default()
    }, || {
        let pkg = support::typecheck_fixture("revive", "example.com/footest", "stutter_bad.go");
        let messages = support::run_analyzer(revive(), &pkg);
        assert!(
            !messages.iter().any(|m| m.contains("stutters")),
            "0.8-confidence stutter hints should be filtered at 0.9: {messages:?}"
        );
    });

    with_settings(Settings {
        confidence: Some(0.1),
        ..Settings::default()
    }, || {
        let pkg = support::typecheck_fixture("revive", "example.com/footest", "stutter_bad.go");
        let messages = support::run_analyzer(revive(), &pkg);
        assert!(
            messages.iter().any(|m| m.contains("stutters")),
            "stutter hints should remain at 0.1: {messages:?}"
        );
    });
}

#[test]
fn revive_skips_generated_files_when_configured() {
    use guff_revive::{with_settings, Settings};

    let pkg = support::typecheck_fixture(
        "revive",
        "example.com/revive/generated",
        "generated_bad.go",
    );
    let without = support::run_analyzer(revive(), &pkg);
    assert!(
        without.iter().any(|m| m.contains("dot-imports:")),
        "generated file should be linted by default: {without:?}"
    );

    with_settings(Settings {
        ignore_generated_header: true,
        ..Settings::default()
    }, || {
        let messages = support::run_analyzer(revive(), &pkg);
        assert!(
            messages.is_empty(),
            "ignore-generated-header should skip generated files: {messages:?}"
        );
    });
}

#[test]
fn revive_context_as_argument_respects_allow_types_before() {
    use guff_revive::{with_settings, RuleArgument, RuleSetting, Settings};
    use std::collections::HashMap;

    let settings = Settings {
        rules: Some(vec![RuleSetting {
            name: "context-as-argument".into(),
            arguments: vec![RuleArgument::Map({
                let mut map = HashMap::new();
                map.insert(
                    "allowTypesBefore".into(),
                    RuleArgument::String("*testing.T,testing.TB".into()),
                );
                map
            })],
            disabled: false,
            severity: None,
        }]),
        ..Settings::default()
    };

    with_settings(settings, || {
        let pkg = support::typecheck_fixture(
            "revive",
            "example.com/revive/ctxallow",
            "context_allow.go",
        );
        let messages = support::run_analyzer(revive(), &pkg);
        assert!(
            !messages.iter().any(|m| m.contains("withTestingT") || m.contains("*testing.T")),
            "allowTypesBefore should permit *testing.T before context: {messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("context-as-argument:")),
            "plain int before context must still be flagged: {messages:?}"
        );
        assert_eq!(
            messages
                .iter()
                .filter(|m| m.contains("context-as-argument:"))
                .count(),
            1,
            "only stillBad should be flagged: {messages:?}"
        );
    });
}

#[test]
fn revive_preserve_scope_suppresses_scope_enlarging_suggestions() {
    use guff_revive::{with_settings, RuleArgument, RuleSetting, Settings};

    let pkg = support::typecheck_fixture(
        "revive",
        "example.com/revive/preservescope",
        "preserve_scope.go",
    );

    // Default (no preserveScope): mid-block if-init + else decls is still reported.
    let without = support::run_analyzer(revive(), &pkg);
    assert!(
        without.iter().any(|m| m.contains("indent-error-flow:")),
        "without preserveScope, mid-block else should be flagged: {without:?}"
    );

    let settings = Settings {
        rules: Some(vec![
            RuleSetting {
                name: "indent-error-flow".into(),
                arguments: vec![RuleArgument::String("preserveScope".into())],
                disabled: false,
                severity: None,
            },
            RuleSetting {
                name: "superfluous-else".into(),
                arguments: vec![RuleArgument::String("preserveScope".into())],
                disabled: false,
                severity: None,
            },
        ]),
        ..Settings::default()
    };

    with_settings(settings, || {
        let messages = support::run_analyzer(revive(), &pkg);
        // Mid-block keepScopeMidBlock suppressed; dropElseAtEnd at block end still fires.
        assert!(
            messages.iter().any(|m| m.contains("indent-error-flow:")),
            "block-end else should still be flagged with preserveScope: {messages:?}"
        );
        assert_eq!(
            messages
                .iter()
                .filter(|m| m.contains("indent-error-flow:"))
                .count(),
            1,
            "only dropElseAtEnd should remain: {messages:?}"
        );
    });
}

#[test]
fn revive_var_naming_skip_initialism_name_checks() {
    use guff_revive::{with_settings, RuleArgument, RuleSetting, Settings};
    use std::collections::HashMap;

    let pkg = support::typecheck_fixture(
        "revive",
        "example.com/revive/varnamingskip",
        "var_naming_skip_initialism.go",
    );

    // Default: initialisms enforced.
    let without = support::run_analyzer(revive(), &pkg);
    assert!(
        without.iter().any(|m| m.contains("var-naming:") && m.contains("HttpRes")),
        "default should flag HttpRes: {without:?}"
    );

    let settings = Settings {
        rules: Some(vec![RuleSetting {
            name: "var-naming".into(),
            arguments: vec![
                RuleArgument::List(Vec::new()),
                RuleArgument::List(Vec::new()),
                RuleArgument::List(vec![RuleArgument::Map({
                    let mut map = HashMap::new();
                    map.insert(
                        "skip-initialism-name-checks".into(),
                        RuleArgument::String("true".into()),
                    );
                    map
                })]),
            ],
            disabled: false,
            severity: None,
        }]),
        ..Settings::default()
    };

    with_settings(settings, || {
        let messages = support::run_analyzer(revive(), &pkg);
        assert!(
            !messages.iter().any(|m| m.contains("var-naming:")),
            "skipInitialismNameChecks should silence initialism warnings: {messages:?}"
        );
    });
}

#[test]
fn revive_var_naming_upper_case_const() {
    use guff_revive::{with_settings, RuleArgument, RuleSetting, Settings};
    use std::collections::HashMap;

    let pkg = support::typecheck_fixture(
        "revive",
        "example.com/revive/varnameupper",
        "var_naming_upper_case_const.go",
    );

    let without = support::run_analyzer(revive(), &pkg);
    assert!(
        without
            .iter()
            .any(|m| m.contains("var-naming:") && m.contains("ALL_CAPS")),
        "default should flag SCREAMING_SNAKE consts: {without:?}"
    );

    let settings = Settings {
        rules: Some(vec![RuleSetting {
            name: "var-naming".into(),
            arguments: vec![
                RuleArgument::List(Vec::new()),
                RuleArgument::List(Vec::new()),
                RuleArgument::List(vec![RuleArgument::Map({
                    let mut map = HashMap::new();
                    map.insert(
                        "upper-case-const".into(),
                        RuleArgument::String("true".into()),
                    );
                    // prometheus also sends this ignored key; must not break parsing.
                    map.insert(
                        "skip-package-name-checks".into(),
                        RuleArgument::String("true".into()),
                    );
                    map
                })]),
            ],
            disabled: false,
            severity: None,
        }]),
        ..Settings::default()
    };

    with_settings(settings, || {
        let messages = support::run_analyzer(revive(), &pkg);
        assert!(
            !messages
                .iter()
                .any(|m| m.contains("SOME_CONST") || m.contains("_SOME_PRIVATE")),
            "upperCaseConst should allow SCREAMING_SNAKE consts: {messages:?}"
        );
        // BAD_VAR_NAME is still ALL_CAPS; message text does not embed the ident.
        assert_eq!(
            messages
                .iter()
                .filter(|m| m.contains("var-naming:") && m.contains("ALL_CAPS"))
                .count(),
            1,
            "non-const ALL_CAPS var should still be flagged once: {messages:?}"
        );
    });
}

#[test]
fn revive_var_naming_allowlist_blocklist() {
    use guff_revive::{with_settings, RuleArgument, RuleSetting, Settings};

    let pkg = support::typecheck_fixture(
        "revive",
        "example.com/revive/varnamelists",
        "var_naming_lists.go",
    );

    let settings = Settings {
        rules: Some(vec![RuleSetting {
            name: "var-naming".into(),
            arguments: vec![
                RuleArgument::List(vec![RuleArgument::String("ID".into())]),
                RuleArgument::List(vec![RuleArgument::String("VM".into())]),
            ],
            disabled: false,
            severity: None,
        }]),
        ..Settings::default()
    };

    with_settings(settings, || {
        let messages = support::run_analyzer(revive(), &pkg);
        assert!(
            !messages.iter().any(|m| m.contains("customId")),
            "allowlist ID should keep customId: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("var-naming:") && m.contains("customVm")),
            "blocklist/common VM should flag customVm: {messages:?}"
        );
    });
}
