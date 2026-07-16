mod support;

use std::sync::Arc;

use guff_analysis::SettingsBag;
use guff_runner::RunnerOptions;
use guff_style::{
    asasalint, asciicheck, bidichk, containedctx, copyloopvar, cyclop, dogsled, exhaustive, exhaustruct, exptostd, forbidigo, funlen,
    gocheckcompilerdirectives, gochecknoglobals, gochecknoinits, gocognit, goconst, gocritic,
    gocyclo, goprintffuncname, iface, inamedparam, interfacebloat, lll, loggercheck, mnd, modernize, musttag,
    nakedret, nestif,
    nlreturn, nosprintfhostport, perfsprint, prealloc, predeclared, reassign, recvcheck, sloglint, tagalign,
    testifylint, thelper, unconvert, usestdlibvars, usetesting, whitespace, wsl,
};

#[test]
fn recvcheck_flags_mixed_receivers() {
    let pkg = support::typecheck_fixture("recvcheck", "example.com/recvcheck", "bad.go");
    let messages = support::run_analyzer(recvcheck(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("the methods of \"RPC\"")
                && m.contains("pointer receiver and non-pointer receiver")),
        "{messages:?}"
    );
    assert_eq!(messages.len(), 1, "{messages:?}");
}

#[test]
fn recvcheck_allows_consistent_and_builtin_unmarshal() {
    let pkg = support::typecheck_fixture("recvcheck", "example.com/recvcheck/ok", "ok.go");
    let messages = support::run_analyzer(recvcheck(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn recvcheck_respects_disable_builtin_and_exclusions() {
    use guff_style::RecvcheckOptions;

    let pkg =
        support::typecheck_fixture("recvcheck", "example.com/recvcheck/settings", "settings.go");

    // Default: UnmarshalJSON excluded → only SQL mixed receivers.
    let flagged = support::run_analyzer(recvcheck(), &pkg);
    assert!(
        flagged.iter().any(|m| m.contains("the methods of \"SQL\"")),
        "{flagged:?}"
    );
    assert!(
        !flagged.iter().any(|m| m.contains("JSON")),
        "UnmarshalJSON should be built-in excluded: {flagged:?}"
    );

    // disable-builtin: JSON also flagged.
    let mut bag = SettingsBag::new();
    bag.insert(
        "recvcheck",
        RecvcheckOptions {
            disable_builtin: true,
            exclusions: Vec::new(),
        },
    );
    let with_disabled = support::run_analyzer_with_settings(
        recvcheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        with_disabled
            .iter()
            .any(|m| m.contains("the methods of \"JSON\"")),
        "{with_disabled:?}"
    );
    assert!(
        with_disabled
            .iter()
            .any(|m| m.contains("the methods of \"SQL\"")),
        "{with_disabled:?}"
    );

    // exclusions: SQL.Value → SQL clean; JSON still excluded by builtin.
    let mut bag = SettingsBag::new();
    bag.insert(
        "recvcheck",
        RecvcheckOptions {
            disable_builtin: false,
            exclusions: vec!["SQL.Value".into()],
        },
    );
    let with_excl = support::run_analyzer_with_settings(
        recvcheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        with_excl.is_empty(),
        "SQL.Value exclusion + builtin Unmarshal should clear all: {with_excl:?}"
    );
}

#[test]
fn iface_flags_identical_interfaces_by_default() {
    let pkg = support::typecheck_fixture("iface", "example.com/iface", "bad.go");
    let messages = support::run_analyzer(iface(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface 'Pinger'") && m.contains("Healthcheck")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface 'Healthcheck'") && m.contains("Pinger")),
        "{messages:?}"
    );
    // Default enable is identical only — Granter unused must not be reported.
    assert!(
        !messages.iter().any(|m| m.contains("Granter")),
        "unused should be off by default: {messages:?}"
    );
    assert_eq!(messages.len(), 2, "{messages:?}");
}

#[test]
fn iface_allows_distinct_and_used_interfaces() {
    let pkg = support::typecheck_fixture("iface", "example.com/iface/ok", "ok.go");
    let messages = support::run_analyzer(iface(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn iface_respects_enable_unused_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::IfaceOptions;

    let pkg = support::typecheck_fixture("iface", "example.com/iface/settings", "settings.go");

    // Default: identical only (Alpha/Beta).
    let flagged = support::run_analyzer(iface(), &pkg);
    assert!(
        flagged.iter().any(|m| m.contains("interface 'Alpha'")),
        "{flagged:?}"
    );
    assert!(
        !flagged.iter().any(|m| m.contains("Orphan")),
        "{flagged:?}"
    );

    // enable unused only: Orphan, not Alpha/Beta identical.
    let mut bag = SettingsBag::new();
    bag.insert(
        "iface",
        IfaceOptions {
            enable: vec!["unused".into()],
            unused_exclude: Vec::new(),
        },
    );
    let messages = support::run_analyzer_with_settings(
        iface(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface 'Orphan'") && m.contains("not used")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("interface 'Alpha'")),
        "identical-only interfaces are also unused: {messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("identical methods") || m.contains("redundancy")),
        "{messages:?}"
    );
}

#[test]
fn thelper_flags_begin_first_name() {
    let pkg = support::typecheck_fixture("thelper", "example.com/thelper", "bad.go");
    let messages = support::run_analyzer(thelper(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("test helper function should start from t.Helper()")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("parameter *testing.T should be the first")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("parameter *testing.T should have name t")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("test helper function should start from b.Helper()")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("parameter *testing.B should have name b")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("test helper function should start from tb.Helper()")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("parameter testing.TB should have name tb")),
        "{messages:?}"
    );
    // anotherCheck is also called from check → not filtered.
    assert!(
        messages
            .iter()
            .any(|m| m.contains("test helper function should start from t.Helper()")),
        "{messages:?}"
    );
    // Anonymous subtest / Test* entry points should not appear alone as false positives.
    assert!(
        !messages.iter().any(|m| m.contains("TestSomething")),
        "{messages:?}"
    );
}

#[test]
fn thelper_allows_valid_helpers_and_filtered_subtests() {
    let pkg = support::typecheck_fixture("thelper", "example.com/thelper/ok", "ok.go");
    let messages = support::run_analyzer(thelper(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn thelper_respects_kind_settings() {
    use guff_style::{ThelperKindOptions, ThelperOptions};

    let pkg = support::typecheck_fixture("thelper", "example.com/thelper/settings", "settings.go");

    // Default: begin reports helperWithoutHelper.
    let flagged = support::run_analyzer(thelper(), &pkg);
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("test helper function should start from t.Helper()")),
        "{flagged:?}"
    );

    // begin off, name on: only wrong name.
    let mut bag = SettingsBag::new();
    bag.insert(
        "thelper",
        ThelperOptions {
            test: ThelperKindOptions {
                first: false,
                name: true,
                begin: false,
            },
            ..ThelperOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        thelper(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("parameter *testing.T should have name t")),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("test helper function should start from t.Helper()")),
        "{messages:?}"
    );
}

#[test]
fn copyloopvar_flags_redundant_copies() {
    let pkg = support::typecheck_fixture("copyloopvar", "example.com/copyloopvar", "bad.go");
    let messages = support::run_analyzer(copyloopvar(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("for") && m.contains("\"i\"")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("for") && m.contains("\"v\"")),
        "{messages:?}"
    );
}

#[test]
fn asasalint_flags_slice_any_as_variadic_any() {
    let pkg = support::typecheck_fixture("asasalint", "example.com/asasalint", "bad.go");
    let messages = support::run_analyzer(asasalint(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("pass []any as any to func A")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("pass []any as any to func errMsg")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("pass []any as any to func B")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("pass []any as any to func Err")),
        "{messages:?}"
    );
    assert!(messages.len() >= 4, "{messages:?}");
}

#[test]
fn asasalint_allows_spread_and_builtin_exclusions() {
    let pkg = support::typecheck_fixture("asasalint", "example.com/asasalint/ok", "ok.go");
    let messages = support::run_analyzer(asasalint(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn asasalint_respects_exclude_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::AsasalintOptions;

    let pkg = support::typecheck_fixture(
        "asasalint",
        "example.com/asasalint/settings",
        "settings.go",
    );

    // Default: Append is reported.
    let flagged = support::run_analyzer(asasalint(), &pkg);
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("pass []any as any to func Append")),
        "{flagged:?}"
    );

    // With exclude: Append is silenced.
    let mut bag = SettingsBag::new();
    bag.insert(
        "asasalint",
        AsasalintOptions {
            exclude: vec!["Append".into()],
            use_builtin_exclusions: false,
        },
    );
    let messages = support::run_analyzer_with_settings(
        asasalint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn reassign_flags_other_package_err_and_eof() {
    let pkg = support::typecheck_fixture("reassign", "example.com/reassign", "bad.go");
    let messages = support::run_analyzer(reassign(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("reassigning variable ErrB in other package b")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("reassigning variable EOF in other package io")),
        "{messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("NotErr")),
        "NotErr should not match default pattern: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("ErrSt")),
        "struct field should not be reported: {messages:?}"
    );
    assert_eq!(messages.len(), 2, "{messages:?}");
}

#[test]
fn reassign_allows_local_and_non_matching() {
    let pkg = support::typecheck_fixture("reassign", "example.com/reassign/ok", "ok.go");
    let messages = support::run_analyzer(reassign(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn reassign_respects_patterns_settings() {
    use guff_style::ReassignOptions;

    let pkg = support::typecheck_fixture("reassign", "example.com/reassign/settings", "settings.go");

    // Default: only ErrB.
    let flagged = support::run_analyzer(reassign(), &pkg);
    assert!(
        flagged
            .iter()
            .any(|m| m.contains("reassigning variable ErrB")),
        "{flagged:?}"
    );
    assert!(
        !flagged.iter().any(|m| m.contains("NotErr")),
        "{flagged:?}"
    );

    // patterns: [".*"] → both.
    let mut bag = SettingsBag::new();
    bag.insert(
        "reassign",
        ReassignOptions {
            patterns: vec![".*".into()],
        },
    );
    let messages = support::run_analyzer_with_settings(
        reassign(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("reassigning variable ErrB")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("reassigning variable NotErr")),
        "{messages:?}"
    );
    assert_eq!(messages.len(), 2, "{messages:?}");
}

#[test]
fn interfacebloat_flags_large_interface() {
    let pkg =
        support::typecheck_fixture("interfacebloat", "example.com/interfacebloat", "bad.go");
    let messages = support::run_analyzer(interfacebloat(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(
        messages[0].contains("the interface has more than 10 methods: 11"),
        "{messages:?}"
    );
}

#[test]
fn interfacebloat_allows_interfaces_within_limit() {
    let pkg =
        support::typecheck_fixture("interfacebloat", "example.com/interfacebloat/ok", "ok.go");
    let messages = support::run_analyzer(interfacebloat(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn interfacebloat_respects_custom_max() {
    use guff_style::InterfacebloatOptions;

    let pkg = support::typecheck_fixture(
        "interfacebloat",
        "example.com/interfacebloat/settings",
        "settings.go",
    );

    // Default max (10): three-method interface is fine.
    assert!(support::run_analyzer(interfacebloat(), &pkg).is_empty());

    // max = 2: the three-method interface is now flagged.
    let mut bag = SettingsBag::new();
    bag.insert("interfacebloat", InterfacebloatOptions { max: 2 });
    let flagged = support::run_analyzer_with_settings(
        interfacebloat(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert!(
        flagged[0].contains("the interface has more than 2 methods: 3"),
        "{flagged:?}"
    );
}

#[test]
fn inamedparam_flags_unnamed_interface_params() {
    let pkg = support::typecheck_fixture("inamedparam", "example.com/inamedparam", "bad.go");
    let messages = support::run_analyzer(inamedparam(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface method SingleParam must have named param for type context.Context")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface method WithoutName must have named param for type context.Context")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface method WithoutName must have named param for type int")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface method WithoutName must have named param for type bool")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface method WithoutName must have all named params")),
        "{messages:?}"
    );
    assert_eq!(messages.len(), 5, "{messages:?}");
}

#[test]
fn inamedparam_allows_named_params() {
    let pkg =
        support::typecheck_fixture("inamedparam", "example.com/inamedparam/ok", "ok.go");
    let messages = support::run_analyzer(inamedparam(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn inamedparam_respects_skip_single_param() {
    use guff_style::InamedparamOptions;

    let pkg = support::typecheck_fixture(
        "inamedparam",
        "example.com/inamedparam/settings",
        "settings.go",
    );

    // Default: single unnamed param is flagged.
    let flagged = support::run_analyzer(inamedparam(), &pkg);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert!(
        flagged[0].contains("interface method Run must have named param for type context.Context"),
        "{flagged:?}"
    );

    // skip-single-param: true → no report.
    let mut bag = SettingsBag::new();
    bag.insert(
        "inamedparam",
        InamedparamOptions {
            skip_single_param: true,
        },
    );
    let skipped = support::run_analyzer_with_settings(
        inamedparam(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(skipped.is_empty(), "unexpected diagnostics: {skipped:?}");
}

#[test]
fn containedctx_flags_context_fields() {
    let pkg = support::typecheck_fixture("containedctx", "example.com/containedctx", "bad.go");
    let messages = support::run_analyzer(containedctx(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("found a struct that contains a context.Context field")),
        "{messages:?}"
    );
}

#[test]
fn containedctx_allows_non_context_fields() {
    let pkg =
        support::typecheck_fixture("containedctx", "example.com/containedctx/ok", "ok.go");
    let messages = support::run_analyzer(containedctx(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn gochecknoinits_flags_init_functions() {
    let pkg = support::typecheck_fixture("gochecknoinits", "example.com/gochecknoinits", "bad.go");
    let messages = support::run_analyzer(gochecknoinits(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages.iter().all(|m| m.contains("`init` function")),
        "{messages:?}"
    );
}

#[test]
fn gochecknoinits_allows_methods_and_other_names() {
    let pkg =
        support::typecheck_fixture("gochecknoinits", "example.com/gochecknoinits/ok", "ok.go");
    assert!(support::run_analyzer(gochecknoinits(), &pkg).is_empty());
}

#[test]
fn gochecknoglobals_flags_package_level_vars() {
    let pkg =
        support::typecheck_fixture("gochecknoglobals", "example.com/gochecknoglobals", "bad.go");
    let messages = support::run_analyzer(gochecknoglobals(), &pkg);
    for name in ["myVar", "myVar1", "myVar2", "Version", "version22", "theVar"] {
        assert!(
            messages
                .iter()
                .any(|m| m.contains(&format!("{name} is a global variable"))),
            "missing {name}: {messages:?}"
        );
    }
    assert_eq!(messages.len(), 6, "{messages:?}");
}

#[test]
fn gochecknoglobals_allows_exceptions() {
    let pkg = support::typecheck_fixture(
        "gochecknoglobals",
        "example.com/gochecknoglobals/ok",
        "ok.go",
    );
    let messages = support::run_analyzer(gochecknoglobals(), &pkg);
    assert!(
        messages.is_empty(),
        "unexpected diagnostics: {messages:?}"
    );
}

#[test]
fn gocheckcompilerdirectives_flags_space_and_unknown() {
    let pkg = support::typecheck_fixture(
        "gocheckcompilerdirectives",
        "example.com/gocheckcompilerdirectives",
        "bad.go",
    );
    let messages = support::run_analyzer(gocheckcompilerdirectives(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("compiler directive contains space: // go:embed")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("compiler directive contains space: //    go:embed")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("compiler directive unrecognized: //go:genrate")),
        "{messages:?}"
    );
    assert_eq!(messages.len(), 3, "{messages:?}");
}

#[test]
fn gocheckcompilerdirectives_allows_valid_directives() {
    let pkg = support::typecheck_fixture(
        "gocheckcompilerdirectives",
        "example.com/gocheckcompilerdirectives/ok",
        "ok.go",
    );
    let messages = support::run_analyzer(gocheckcompilerdirectives(), &pkg);
    assert!(
        messages.is_empty(),
        "unexpected diagnostics: {messages:?}"
    );
}

#[test]
fn forbidigo_flags_default_print_patterns() {
    let pkg = support::typecheck_fixture("forbidigo", "example.com/forbidigo", "bad.go");
    let messages = support::run_analyzer(forbidigo(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("use of `fmt.Println` forbidden")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("use of `fmt.Printf` forbidden")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("use of `print` forbidden")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("use of `println` forbidden")),
        "{messages:?}"
    );
    assert_eq!(messages.len(), 4, "{messages:?}");
}

#[test]
fn forbidigo_allows_sprintf() {
    let pkg = support::typecheck_fixture("forbidigo", "example.com/forbidigo/ok", "ok.go");
    let messages = support::run_analyzer(forbidigo(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn forbidigo_respects_custom_forbid_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::{ForbidigoOptions, ForbidigoPattern};

    let pkg = support::typecheck_fixture("forbidigo", "example.com/forbidigo/custom", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "forbidigo",
        ForbidigoOptions {
            forbid: vec![ForbidigoPattern {
                pattern: r"^fmt\.Print.*$".into(),
                pkg: String::new(),
                msg: "Do not commit print statements.".into(),
            }],
            exclude_godoc_examples: true,
            analyze_types: false,
        },
    );
    let messages = support::run_analyzer_with_settings(
        forbidigo(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| {
            m.contains("use of `fmt.Println` forbidden")
                && m.contains("Do not commit print statements.")
        }),
        "{messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("`print`")),
        "builtin print should not match custom fmt-only pattern: {messages:?}"
    );
}

#[test]
fn bidichk_flags_dangerous_unicode_in_source() {
    let pkg = support::typecheck_fixture("bidichk", "example.com/bidichk", "bad.go");
    let messages = support::run_analyzer(bidichk(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("RIGHT-TO-LEFT-OVERRIDE")),
        "{messages:?}"
    );
    assert!(!messages.is_empty(), "{messages:?}");
}

#[test]
fn bidichk_allows_clean_source() {
    let pkg = support::typecheck_fixture("bidichk/ok", "example.com/bidichk/ok", "ok.go");
    assert!(support::run_analyzer(bidichk(), &pkg).is_empty());
}

#[test]
fn bidichk_respects_disallowed_runes_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::BidichkOptions;

    let pkg = support::typecheck_fixture("bidichk/settings", "example.com/bidichk/settings", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "bidichk",
        BidichkOptions {
            disallowed_runes: vec!["LEFT-TO-RIGHT-OVERRIDE".into()],
        },
    );
    let messages = support::run_analyzer_with_settings(
        bidichk(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("LEFT-TO-RIGHT-OVERRIDE")),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("RIGHT-TO-LEFT-OVERRIDE")),
        "RLO should be skipped when only LRO is enabled: {messages:?}"
    );
}

#[test]
fn copyloopvar_allows_alias_copies() {
    let pkg = support::typecheck_fixture("copyloopvar", "example.com/copyloopvar/ok", "ok.go");
    assert!(support::run_analyzer(copyloopvar(), &pkg).is_empty());
}

#[test]
fn usetesting_flags_os_mkdirtemp_and_createtemp() {
    let pkg = support::typecheck_fixture("usetesting", "example.com/usetesting", "bad.go");
    let messages = support::run_analyzer(usetesting(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("os.MkdirTemp") && m.contains("t.TempDir")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("os.CreateTemp") && m.contains("t.TempDir")),
        "{messages:?}"
    );
}

#[test]
fn usetesting_allows_testing_helpers() {
    let pkg = support::typecheck_fixture("usetesting", "example.com/usetesting/ok", "ok.go");
    assert!(support::run_analyzer(usetesting(), &pkg).is_empty());
}

#[test]
fn usestdlibvars_flags_http_literals() {
    let pkg = support::typecheck_fixture("usestdlibvars", "example.com/usestdlibvars", "bad.go");
    let messages = support::run_analyzer(usestdlibvars(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("http.MethodGet")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("http.StatusNotFound")),
        "{messages:?}"
    );
}

#[test]
fn usestdlibvars_allows_stdlib_constants() {
    let pkg = support::typecheck_fixture("usestdlibvars", "example.com/usestdlibvars/ok", "ok.go");
    assert!(support::run_analyzer(usestdlibvars(), &pkg).is_empty());
}

#[test]
fn copyloopvar_check_alias_flags_renames() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::CopyloopvarOptions;

    let pkg = support::typecheck_fixture("copyloopvar", "example.com/copyloopvar/ok", "ok.go");
    assert!(
        support::run_analyzer(copyloopvar(), &pkg).is_empty(),
        "default check-alias=false should allow alias copies"
    );

    let mut bag = SettingsBag::new();
    bag.insert("copyloopvar", CopyloopvarOptions { check_alias: true });
    let messages = support::run_analyzer_with_settings(
        copyloopvar(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("for") && m.contains("\"i\"")),
        "check-alias=true should flag alias copies: {messages:?}"
    );
}

#[test]
fn usetesting_respects_os_setenv_and_temp_dir() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::UsetestingOptions;

    let pkg = support::typecheck_fixture(
        "usetesting",
        "example.com/usetesting/settings",
        "settings_extra.go",
    );
    assert!(
        support::run_analyzer(usetesting(), &pkg).is_empty(),
        "defaults should ignore Setenv/TempDir: {:?}",
        support::run_analyzer(usetesting(), &pkg)
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "usetesting",
        UsetestingOptions {
            os_setenv: true,
            os_temp_dir: true,
            ..UsetestingOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        usetesting(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("os.Setenv") && m.contains("t.Setenv")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("os.TempDir") && m.contains("t.TempDir")),
        "{messages:?}"
    );
}

#[test]
fn usetesting_respects_os_mkdir_temp_off() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::UsetestingOptions;

    let pkg = support::typecheck_fixture("usetesting", "example.com/usetesting", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "usetesting",
        UsetestingOptions {
            os_mkdir_temp: false,
            os_create_temp: false,
            ..UsetestingOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        usetesting(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "os-mkdir-temp/os-create-temp=false should suppress bad.go: {messages:?}"
    );
}

#[test]
fn usestdlibvars_respects_http_toggles_off() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::UsestdlibvarsOptions;

    let pkg = support::typecheck_fixture("usestdlibvars", "example.com/usestdlibvars", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "usestdlibvars",
        UsestdlibvarsOptions {
            http_method: false,
            http_status_code: false,
            ..UsestdlibvarsOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        usestdlibvars(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "http toggles off should suppress bad.go: {messages:?}"
    );
}

#[test]
fn usestdlibvars_optional_tables_default_off() {
    let pkg = support::typecheck_fixture(
        "usestdlibvars",
        "example.com/usestdlibvars/optional",
        "optional_bad.go",
    );
    let messages = support::run_analyzer(usestdlibvars(), &pkg);
    assert!(
        messages.is_empty(),
        "optional tables default off: {messages:?}"
    );
}

#[test]
fn usestdlibvars_optional_tables_when_enabled() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::UsestdlibvarsOptions;

    let pkg = support::typecheck_fixture(
        "usestdlibvars",
        "example.com/usestdlibvars/optional",
        "optional_bad.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "usestdlibvars",
        UsestdlibvarsOptions {
            http_method: false,
            http_status_code: false,
            time_weekday: true,
            time_month: true,
            time_layout: true,
            crypto_hash: true,
            default_rpc_path: true,
            sql_isolation_level: true,
            tls_signature_scheme: true,
            constant_kind: true,
            time_date_month: true,
        },
    );
    let messages = support::run_analyzer_with_settings(
        usestdlibvars(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    for needle in [
        "\"Monday\" can be replaced by time.Monday.String()",
        "\"January\" can be replaced by time.January.String()",
        "\"2006-01-02\" can be replaced by time.DateOnly",
        "\"SHA-256\" can be replaced by crypto.SHA256.String()",
        "\"/_goRPC_\" can be replaced by rpc.DefaultRPCPath",
        "\"Read Committed\" can be replaced by sql.LevelReadCommitted.String()",
        "\"PSSWithSHA256\" can be replaced by tls.PSSWithSHA256.String()",
        "\"Bool\" can be replaced by constant.Bool.String()",
        "\"1\" can be replaced by time.January",
    ] {
        assert!(
            messages.iter().any(|m| m.contains(needle)),
            "missing {needle} in {messages:?}"
        );
    }
}

#[test]
fn usestdlibvars_optional_ok_clean() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::UsestdlibvarsOptions;

    let pkg = support::typecheck_fixture(
        "usestdlibvars",
        "example.com/usestdlibvars/optional_ok",
        "optional_ok.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "usestdlibvars",
        UsestdlibvarsOptions {
            http_method: false,
            http_status_code: false,
            time_weekday: true,
            time_month: true,
            time_layout: true,
            crypto_hash: true,
            default_rpc_path: true,
            sql_isolation_level: true,
            tls_signature_scheme: true,
            constant_kind: true,
            time_date_month: true,
        },
    );
    let messages = support::run_analyzer_with_settings(
        usestdlibvars(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "stdlib constants should be clean: {messages:?}"
    );
}

#[test]
fn perfsprint_flags_fmt_shortcuts() {
    let pkg = support::typecheck_fixture("perfsprint", "example.com/perfsprint", "bad.go");
    let messages = support::run_analyzer(perfsprint(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("string-format") && m.contains("fmt.Sprintf")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("error-format") && m.contains("errors.New")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("bool-format") && m.contains("FormatBool")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("integer-format") && m.contains("Itoa")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("hex-format") && m.contains("EncodeToString")),
        "{messages:?}"
    );
}

#[test]
fn perfsprint_allows_complex_fmt() {
    let pkg = support::typecheck_fixture("perfsprint", "example.com/perfsprint/ok", "ok.go");
    assert!(support::run_analyzer(perfsprint(), &pkg).is_empty());
}

#[test]
fn perfsprint_flags_concat_loop() {
    let pkg = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/concat",
        "concat_loop_bad.go",
    );
    let messages = support::run_analyzer(perfsprint(), &pkg);
    let concat: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("concat-loop"))
        .collect();
    assert!(
        concat.len() >= 8,
        "expected several concat-loop diagnostics, got {}: {messages:?}",
        concat.len()
    );
    assert!(
        concat
            .iter()
            .all(|m| m.contains("string concatenation in a loop")),
        "{concat:?}"
    );
}

#[test]
fn perfsprint_concat_loop_allows_local_and_other_ops() {
    let pkg = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/concat_ok",
        "concat_loop_ok.go",
    );
    let messages = support::run_analyzer(perfsprint(), &pkg);
    assert!(
        !messages.iter().any(|m| m.contains("concat-loop")),
        "default loop-other-ops=false should skip otherOps cases; locals should be ignored: {messages:?}"
    );
}

#[test]
fn perfsprint_concat_loop_respects_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PerfsprintOptions;

    let bad = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/concat_settings",
        "concat_loop_bad.go",
    );
    let ok = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/concat_ok_settings",
        "concat_loop_ok.go",
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "perfsprint",
        PerfsprintOptions {
            concat_loop: false,
            ..PerfsprintOptions::default()
        },
    );
    let disabled = support::run_analyzer_with_settings(
        perfsprint(),
        &bad,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !disabled.iter().any(|m| m.contains("concat-loop")),
        "concat-loop=false should suppress: {disabled:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "perfsprint",
        PerfsprintOptions {
            loop_other_ops: true,
            ..PerfsprintOptions::default()
        },
    );
    let with_other = support::run_analyzer_with_settings(
        perfsprint(),
        &ok,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        with_other.iter().any(|m| m.contains("concat-loop")),
        "loop-other-ops=true should report otherOps concat loops: {with_other:?}"
    );
}

#[test]
fn goconst_flags_repeated_strings() {
    let pkg = support::typecheck_fixture("goconst", "example.com/goconst", "bad.go");
    let messages = support::run_analyzer(goconst(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("needconst") && m.contains("3 occurrences")),
        "{messages:?}"
    );
}

#[test]
fn goconst_allows_below_threshold() {
    let pkg = support::typecheck_fixture("goconst", "example.com/goconst/ok", "ok.go");
    assert!(support::run_analyzer(goconst(), &pkg).is_empty());
}

#[test]
fn goconst_flags_repeated_numbers() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GoconstOptions;

    let pkg =
        support::typecheck_fixture("goconst", "example.com/goconst/numbers", "numbers_bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "goconst",
        GoconstOptions {
            numbers: true,
            number_min: 0,
            number_max: 0,
            ..GoconstOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        goconst(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("`100`") && m.contains("3 occurrences")),
        "{messages:?}"
    );
}

#[test]
fn goconst_numbers_respect_range_and_threshold() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GoconstOptions;

    let pkg =
        support::typecheck_fixture("goconst", "example.com/goconst/numbers_ok", "numbers_ok.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "goconst",
        GoconstOptions {
            numbers: true,
            number_min: 0,
            number_max: 0,
            ..GoconstOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        goconst(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "numbers_ok.go should stay clean: {messages:?}"
    );
}

#[test]
fn goconst_match_constant_reports_existing_const() {
    let pkg = support::typecheck_fixture(
        "goconst",
        "example.com/goconst/match",
        "match_constant_bad.go",
    );
    let messages = support::run_analyzer(goconst(), &pkg);
    assert!(
        messages.iter().any(|m| {
            m.contains("repeated value")
                && m.contains("3 occurrences")
                && m.contains("ExistingConst")
        }),
        "{messages:?}"
    );
}

#[test]
fn goconst_match_constant_allows_below_threshold() {
    let pkg = support::typecheck_fixture(
        "goconst",
        "example.com/goconst/match_ok",
        "match_constant_ok.go",
    );
    assert!(support::run_analyzer(goconst(), &pkg).is_empty());
}

#[test]
fn goconst_find_duplicates_reports_duplicate_consts() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GoconstOptions;

    let pkg = support::typecheck_fixture(
        "goconst",
        "example.com/goconst/find_dup",
        "find_duplicates_bad.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "goconst",
        GoconstOptions {
            find_duplicates: true,
            match_constant: false,
            ..GoconstOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        goconst(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| {
            m.contains("This constant is a duplicate of `DuplicateConst1`")
                && m.contains("find_duplicates_bad.go")
        }),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| { m.contains("This constant is a duplicate of `GroupedDuplicateConst1`") }),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| { m.contains("This constant is a duplicate of `ScopedDuplicateConst1`") }),
        "{messages:?}"
    );
}

#[test]
fn goconst_find_duplicates_default_off() {
    let pkg = support::typecheck_fixture(
        "goconst",
        "example.com/goconst/find_dup_default",
        "find_duplicates_bad.go",
    );
    let messages = support::run_analyzer(goconst(), &pkg);
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("This constant is a duplicate")),
        "find-duplicates defaults to false: {messages:?}"
    );
}

#[test]
fn goconst_find_duplicates_allows_unique_consts() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GoconstOptions;

    let pkg = support::typecheck_fixture(
        "goconst",
        "example.com/goconst/find_dup_ok",
        "find_duplicates_ok.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "goconst",
        GoconstOptions {
            find_duplicates: true,
            match_constant: false,
            ..GoconstOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        goconst(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "find_duplicates_ok.go should stay clean: {messages:?}"
    );
}

#[test]
fn dogsled_flags_too_many_blanks() {
    let pkg = support::typecheck_fixture("dogsled", "example.com/dogsled", "bad.go");
    let messages = support::run_analyzer(dogsled(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("declaration has 3 blank identifiers")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("declaration has 4 blank identifiers")),
        "{messages:?}"
    );
}

#[test]
fn dogsled_allows_two_or_fewer_blanks() {
    let pkg = support::typecheck_fixture("dogsled", "example.com/dogsled/ok", "ok.go");
    assert!(support::run_analyzer(dogsled(), &pkg).is_empty());
}

#[test]
fn asciicheck_flags_non_ascii_idents() {
    let pkg = support::typecheck_fixture("asciicheck", "example.com/asciicheck", "bad.go");
    let messages = support::run_analyzer(asciicheck(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("TéstFunc") && m.contains("non-ASCII")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("téstConst") && m.contains("non-ASCII")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("téstParam") && m.contains("non-ASCII")),
        "{messages:?}"
    );
}

#[test]
fn asciicheck_allows_ascii_idents() {
    let pkg = support::typecheck_fixture("asciicheck", "example.com/asciicheck/ok", "ok.go");
    assert!(support::run_analyzer(asciicheck(), &pkg).is_empty());
}

#[test]
fn goprintffuncname_flags_missing_f_suffix() {
    let pkg =
        support::typecheck_fixture("goprintffuncname", "example.com/goprintffuncname", "bad.go");
    let messages = support::run_analyzer(goprintffuncname(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("prinfLikeFunc") && m.contains("prinfLikeFuncf")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("prinfLikeFuncAny") && m.contains("should be named")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("prinfLikeFuncWithExtraArgs")),
        "{messages:?}"
    );
}

#[test]
fn goprintffuncname_allows_correct_names() {
    let pkg = support::typecheck_fixture(
        "goprintffuncname",
        "example.com/goprintffuncname/ok",
        "ok.go",
    );
    assert!(support::run_analyzer(goprintffuncname(), &pkg).is_empty());
}

#[test]
fn funlen_flags_too_many_statements() {
    let pkg = support::typecheck_fixture("funlen", "example.com/funlen", "bad.go");
    let messages = support::run_analyzer(funlen(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("TooManyStatements") && m.contains("too many statements")),
        "{messages:?}"
    );
}

#[test]
fn funlen_allows_short_functions() {
    let pkg = support::typecheck_fixture("funlen", "example.com/funlen/ok", "ok.go");
    assert!(support::run_analyzer(funlen(), &pkg).is_empty());
}

#[test]
fn gocyclo_flags_high_complexity() {
    let pkg = support::typecheck_fixture("gocyclo", "example.com/gocyclo", "bad.go");
    let messages = support::run_analyzer(gocyclo(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("HighComplexity") && m.contains("cyclomatic complexity")),
        "{messages:?}"
    );
}

#[test]
fn gocyclo_allows_low_complexity() {
    let pkg = support::typecheck_fixture("gocyclo", "example.com/gocyclo/ok", "ok.go");
    assert!(support::run_analyzer(gocyclo(), &pkg).is_empty());
}

#[test]
fn lll_flags_long_lines() {
    let pkg = support::typecheck_fixture("lll", "example.com/lll", "bad.go");
    let messages = support::run_analyzer(lll(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("characters long") && m.contains("120")),
        "{messages:?}"
    );
}

#[test]
fn lll_allows_short_lines() {
    let pkg = support::typecheck_fixture("lll", "example.com/lll/ok", "ok.go");
    assert!(support::run_analyzer(lll(), &pkg).is_empty());
}

#[test]
fn gocognit_flags_high_cognitive_complexity() {
    let pkg = support::typecheck_fixture("gocognit", "example.com/gocognit", "bad.go");
    let messages = support::run_analyzer(gocognit(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("HighCognitive") && m.contains("cognitive complexity")),
        "{messages:?}"
    );
}

#[test]
fn gocognit_allows_low_cognitive_complexity() {
    let pkg = support::typecheck_fixture("gocognit", "example.com/gocognit/ok", "ok.go");
    assert!(support::run_analyzer(gocognit(), &pkg).is_empty());
}

#[test]
fn nestif_flags_deep_nesting() {
    let pkg = support::typecheck_fixture("nestif", "example.com/nestif", "bad.go");
    let messages = support::run_analyzer(nestif(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("complex nested blocks") && m.contains("if a")),
        "{messages:?}"
    );
}

#[test]
fn nestif_allows_shallow_nesting() {
    let pkg = support::typecheck_fixture("nestif", "example.com/nestif/ok", "ok.go");
    assert!(support::run_analyzer(nestif(), &pkg).is_empty());
}

#[test]
fn cyclop_flags_high_complexity() {
    let pkg = support::typecheck_fixture("cyclop", "example.com/cyclop", "bad.go");
    let messages = support::run_analyzer(cyclop(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("HighComplexity") && m.contains("cyclomatic complexity")),
        "{messages:?}"
    );
}

#[test]
fn cyclop_allows_low_complexity() {
    let pkg = support::typecheck_fixture("cyclop", "example.com/cyclop/ok", "ok.go");
    assert!(support::run_analyzer(cyclop(), &pkg).is_empty());
}

#[test]
fn nakedret_flags_long_naked_returns() {
    let pkg = support::typecheck_fixture("nakedret", "example.com/nakedret", "bad.go");
    let messages = support::run_analyzer(nakedret(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("naked return") && m.contains("LongNamed")),
        "{messages:?}"
    );
}

#[test]
fn nakedret_allows_short_or_explicit() {
    let pkg = support::typecheck_fixture("nakedret", "example.com/nakedret/ok", "ok.go");
    assert!(support::run_analyzer(nakedret(), &pkg).is_empty());
}

#[test]
fn nosprintfhostport_flags_host_port_sprintf() {
    let pkg = support::typecheck_fixture(
        "nosprintfhostport",
        "example.com/nosprintfhostport",
        "bad.go",
    );
    let messages = support::run_analyzer(nosprintfhostport(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("net.JoinHostPort") && m.contains("fmt.Sprintf")),
        "{messages:?}"
    );
    assert!(
        messages.len() >= 2,
        "expected both host:port and auth URL hits, got {messages:?}"
    );
}

#[test]
fn nosprintfhostport_allows_safe_sprintf() {
    let pkg = support::typecheck_fixture(
        "nosprintfhostport",
        "example.com/nosprintfhostport/ok",
        "ok.go",
    );
    assert!(support::run_analyzer(nosprintfhostport(), &pkg).is_empty());
}

#[test]
fn predeclared_flags_shadowed_identifiers() {
    let pkg = support::typecheck_fixture("predeclared", "example.com/predeclared", "bad.go");
    let messages = support::run_analyzer(predeclared(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("function len") && m.contains("predeclared")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("variable error") && m.contains("predeclared")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("variable true") && m.contains("predeclared")),
        "{messages:?}"
    );
}

#[test]
fn predeclared_allows_non_shadowing_names() {
    let pkg = support::typecheck_fixture("predeclared", "example.com/predeclared/ok", "ok.go");
    assert!(support::run_analyzer(predeclared(), &pkg).is_empty());
}

#[test]
fn whitespace_flags_leading_and_trailing() {
    let pkg = support::typecheck_fixture("whitespace", "example.com/whitespace", "bad.go");
    let messages = support::run_analyzer(whitespace(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("unnecessary leading newline")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("unnecessary trailing newline")),
        "{messages:?}"
    );
}

#[test]
fn whitespace_allows_tight_blocks() {
    let pkg = support::typecheck_fixture("whitespace", "example.com/whitespace/ok", "ok.go");
    assert!(support::run_analyzer(whitespace(), &pkg).is_empty());
}

#[test]
fn whitespace_multi_if_requires_leading_newline_when_enabled() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::WhitespaceOptions;

    let pkg = support::typecheck_fixture(
        "whitespace",
        "example.com/whitespace/multiif",
        "multi_if_bad.go",
    );
    assert!(
        support::run_analyzer(whitespace(), &pkg).is_empty(),
        "multi-if off should not flag multi_if_bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "whitespace",
        WhitespaceOptions {
            multi_if: true,
            ..WhitespaceOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        whitespace(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("multi-line statement should be followed by a newline")),
        "multi-if=true should flag multi_if_bad.go: {messages:?}"
    );

    let ok_pkg = support::typecheck_fixture(
        "whitespace",
        "example.com/whitespace/multiif/ok",
        "multi_if_ok.go",
    );
    let mut ok_bag = SettingsBag::new();
    ok_bag.insert(
        "whitespace",
        WhitespaceOptions {
            multi_if: true,
            ..WhitespaceOptions::default()
        },
    );
    let ok_messages = support::run_analyzer_with_settings(
        whitespace(),
        &ok_pkg,
        &RunnerOptions {
            settings: Arc::new(ok_bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        ok_messages.is_empty(),
        "multi-if=true should allow multi_if_ok.go: {ok_messages:?}"
    );
}

#[test]
fn whitespace_multi_func_requires_leading_newline_when_enabled() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::WhitespaceOptions;

    let pkg = support::typecheck_fixture(
        "whitespace",
        "example.com/whitespace/multifunc",
        "multi_func_bad.go",
    );
    assert!(
        support::run_analyzer(whitespace(), &pkg).is_empty(),
        "multi-func off should not flag multi_func_bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "whitespace",
        WhitespaceOptions {
            multi_func: true,
            ..WhitespaceOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        whitespace(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("multi-line statement should be followed by a newline")),
        "multi-func=true should flag multi_func_bad.go: {messages:?}"
    );

    let ok_pkg = support::typecheck_fixture(
        "whitespace",
        "example.com/whitespace/multifunc/ok",
        "multi_func_ok.go",
    );
    let mut ok_bag = SettingsBag::new();
    ok_bag.insert(
        "whitespace",
        WhitespaceOptions {
            multi_func: true,
            ..WhitespaceOptions::default()
        },
    );
    let ok_messages = support::run_analyzer_with_settings(
        whitespace(),
        &ok_pkg,
        &RunnerOptions {
            settings: Arc::new(ok_bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        ok_messages.is_empty(),
        "multi-func=true should allow multi_func_ok.go: {ok_messages:?}"
    );
}

#[test]
fn nlreturn_flags_missing_blank_before_return() {
    let pkg = support::typecheck_fixture("nlreturn", "example.com/nlreturn", "bad.go");
    let messages = support::run_analyzer(nlreturn(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("return with no blank line before")),
        "{messages:?}"
    );
}

#[test]
fn nlreturn_allows_alone_or_blanked_returns() {
    let pkg = support::typecheck_fixture("nlreturn", "example.com/nlreturn/ok", "ok.go");
    assert!(support::run_analyzer(nlreturn(), &pkg).is_empty());
}

#[test]
fn mnd_flags_magic_numbers() {
    let pkg = support::typecheck_fixture("mnd", "example.com/mnd", "bad.go");
    let messages = support::run_analyzer(mnd(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Magic number") && m.contains("<condition>")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Magic number") && m.contains("<argument>")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Magic number") && m.contains("<return>")),
        "{messages:?}"
    );
}

#[test]
fn mnd_allows_ignored_literals() {
    let pkg = support::typecheck_fixture("mnd", "example.com/mnd/ok", "ok.go");
    assert!(support::run_analyzer(mnd(), &pkg).is_empty());
}

#[test]
fn prealloc_flags_range_append() {
    let pkg = support::typecheck_fixture("prealloc", "example.com/prealloc", "bad.go");
    let messages = support::run_analyzer(prealloc(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Consider preallocating dest")),
        "{messages:?}"
    );
}

#[test]
fn prealloc_allows_make_capacity() {
    let pkg = support::typecheck_fixture("prealloc", "example.com/prealloc/ok", "ok.go");
    assert!(support::run_analyzer(prealloc(), &pkg).is_empty());
}

#[test]
fn tagalign_flags_misaligned_tags() {
    let pkg = support::typecheck_fixture("tagalign", "example.com/tagalign", "bad.go");
    let messages = support::run_analyzer(tagalign(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("tag is not aligned")),
        "{messages:?}"
    );
}

#[test]
fn tagalign_allows_aligned_sorted_tags() {
    let pkg = support::typecheck_fixture("tagalign", "example.com/tagalign/ok", "ok.go");
    assert!(support::run_analyzer(tagalign(), &pkg).is_empty());
}

#[test]
fn wsl_flags_cuddle_violations() {
    let pkg = support::typecheck_fixture("wsl", "example.com/wsl", "bad.go");
    let messages = support::run_analyzer(wsl(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("if statements should only be cuddled with assignments")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("used in the if statement itself")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("only one cuddle assignment allowed before if")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("declarations should never be cuddled")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("assignments should only be cuddled with other assignments")),
        "{messages:?}"
    );
}

#[test]
fn wsl_allows_proper_spacing() {
    let pkg = support::typecheck_fixture("wsl", "example.com/wsl/ok", "ok.go");
    let messages = support::run_analyzer(wsl(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn gocyclo_respects_min_complexity_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GocycloOptions;

    let pkg = support::typecheck_fixture("gocyclo", "example.com/gocyclo", "bad.go");
    assert!(
        !support::run_analyzer(gocyclo(), &pkg).is_empty(),
        "default min-complexity=30 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert("gocyclo", GocycloOptions { min_complexity: 50 });
    let messages = support::run_analyzer_with_settings(
        gocyclo(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "min-complexity=50 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn dogsled_respects_max_blank_identifiers_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::DogsledOptions;

    let pkg = support::typecheck_fixture("dogsled", "example.com/dogsled", "bad.go");
    assert!(
        !support::run_analyzer(dogsled(), &pkg).is_empty(),
        "default max-blank-identifiers=2 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "dogsled",
        DogsledOptions {
            max_blank_identifiers: 4,
        },
    );
    let messages = support::run_analyzer_with_settings(
        dogsled(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "max-blank-identifiers=4 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn funlen_respects_statements_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::FunlenOptions;

    let pkg = support::typecheck_fixture("funlen", "example.com/funlen", "bad.go");
    assert!(
        !support::run_analyzer(funlen(), &pkg).is_empty(),
        "default statements=40 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "funlen",
        FunlenOptions {
            statements: 50,
            ..FunlenOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        funlen(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "statements=50 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn cyclop_respects_max_complexity_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::CyclopOptions;

    let pkg = support::typecheck_fixture("cyclop", "example.com/cyclop", "bad.go");
    assert!(
        !support::run_analyzer(cyclop(), &pkg).is_empty(),
        "default max-complexity=10 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "cyclop",
        CyclopOptions {
            max_complexity: 20,
            ..CyclopOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        cyclop(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "max-complexity=20 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn lll_respects_line_length_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::LllOptions;

    let pkg = support::typecheck_fixture("lll", "example.com/lll", "bad.go");
    assert!(
        !support::run_analyzer(lll(), &pkg).is_empty(),
        "default line-length=120 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "lll",
        LllOptions {
            line_length: 200,
            ..LllOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        lll(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "line-length=200 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn nakedret_respects_max_func_lines_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::NakedretOptions;

    let pkg = support::typecheck_fixture("nakedret", "example.com/nakedret", "bad.go");
    assert!(
        !support::run_analyzer(nakedret(), &pkg).is_empty(),
        "default max-func-lines=30 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "nakedret",
        NakedretOptions {
            max_func_lines: 50,
            ..NakedretOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        nakedret(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "max-func-lines=50 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn nlreturn_respects_block_size_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::NlreturnOptions;

    let pkg = support::typecheck_fixture("nlreturn", "example.com/nlreturn", "bad.go");
    assert!(
        !support::run_analyzer(nlreturn(), &pkg).is_empty(),
        "default block-size=1 should flag bad.go"
    );

    let mut bag = SettingsBag::new();
    bag.insert("nlreturn", NlreturnOptions { block_size: 10 });
    let messages = support::run_analyzer_with_settings(
        nlreturn(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "block-size=10 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn cyclop_respects_package_average_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::CyclopOptions;

    let pkg = support::typecheck_fixture("cyclop", "example.com/cyclop/pkgavg", "pkgavg_bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "cyclop",
        CyclopOptions {
            max_complexity: 20,
            package_average: 5.0,
            ..CyclopOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        cyclop(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("average complexity for the package")),
        "package-average=5 should flag pkgavg_bad.go: {messages:?}"
    );
}

#[test]
fn nakedret_skips_test_files_when_configured() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::NakedretOptions;

    let pkg = support::typecheck_with_deps(
        "example.com/nakedret/test",
        &support::testdata("nakedret/bad_test.go"),
        &[],
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "nakedret",
        NakedretOptions {
            skip_test_files: true,
            ..NakedretOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        nakedret(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "skip-test-files should ignore bad_test.go: {messages:?}"
    );
}

#[test]
fn perfsprint_respects_disabled_integer_format() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PerfsprintOptions;

    let pkg = support::typecheck_fixture("perfsprint", "example.com/perfsprint", "bad.go");
    assert!(
        support::run_analyzer(perfsprint(), &pkg)
            .iter()
            .any(|m| m.contains("integer-format")),
        "default should flag integer-format"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "perfsprint",
        PerfsprintOptions {
            integer_format: false,
            bool_format: false,
            hex_format: false,
            ..PerfsprintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        perfsprint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages.iter().any(|m| m.contains("integer-format")),
        "integer-format=false should suppress integer diagnostics: {messages:?}"
    );
}

#[test]
fn perfsprint_err_error_off_by_default() {
    let pkg = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/err_error",
        "err_error.go",
    );
    let messages = support::run_analyzer(perfsprint(), &pkg);
    assert!(
        !messages.iter().any(|m| m.contains(".Error()")),
        "err-error defaults to false: {messages:?}"
    );
}

#[test]
fn perfsprint_err_error_when_enabled() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PerfsprintOptions;

    let pkg = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/err_error_on",
        "err_error.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "perfsprint",
        PerfsprintOptions {
            err_error: true,
            ..PerfsprintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        perfsprint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("error-format") && m.contains("err.Error()")),
        "err-error=true should suggest err.Error(): {messages:?}"
    );
    assert_eq!(
        messages.iter().filter(|m| m.contains(".Error()")).count(),
        3,
        "expected Sprint/Sprintf %s/%v: {messages:?}"
    );
}

#[test]
fn perfsprint_int_conversion_when_disabled() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PerfsprintOptions;

    let pkg = support::typecheck_fixture(
        "perfsprint",
        "example.com/perfsprint/int_conv",
        "int_conversion.go",
    );

    // Default (int-conversion=true): cast-requiring and non-cast types.
    let default_msgs = support::run_analyzer(perfsprint(), &pkg);
    assert!(
        default_msgs.iter().any(|m| m.contains("Itoa")),
        "{default_msgs:?}"
    );
    assert!(
        default_msgs.iter().any(|m| m.contains("FormatUint")),
        "{default_msgs:?}"
    );
    assert_eq!(
        default_msgs
            .iter()
            .filter(|m| m.contains("integer-format"))
            .count(),
        5,
        "int/int8/int64/uint/uint64: {default_msgs:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "perfsprint",
        PerfsprintOptions {
            int_conversion: false,
            ..PerfsprintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        perfsprint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    // int and int64/uint64 need no cast — still flagged.
    assert!(
        messages.iter().any(|m| m.contains("strconv.Itoa")),
        "plain int should still flag: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("FormatInt")),
        "int64 should still flag: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("FormatUint") && !m.contains("uint64(")),
        "uint64 should still flag without cast: {messages:?}"
    );
    // int8 / uint require casts — suppressed.
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("integer-format"))
            .count(),
        3,
        "int-conversion=false should keep only int/int64/uint64: {messages:?}"
    );
}

#[test]
fn goconst_respects_min_occurrences_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GoconstOptions;

    let pkg = support::typecheck_fixture("goconst", "example.com/goconst", "bad.go");
    assert!(!support::run_analyzer(goconst(), &pkg).is_empty());

    let mut bag = SettingsBag::new();
    bag.insert(
        "goconst",
        GoconstOptions {
            min_occurrences: 10,
            ..GoconstOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        goconst(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "min-occurrences=10 should suppress bad.go: {messages:?}"
    );
}

#[test]
fn predeclared_respects_ignore_setting() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PredeclaredOptions;

    let pkg = support::typecheck_fixture("predeclared", "example.com/predeclared", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "predeclared",
        PredeclaredOptions {
            ignore: vec!["len".into(), "error".into(), "true".into()],
            ..PredeclaredOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        predeclared(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "ignore list should suppress bad.go: {messages:?}"
    );
}

#[test]
fn mnd_respects_disabled_argument_check() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::MndOptions;

    let pkg = support::typecheck_fixture("mnd", "example.com/mnd", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "mnd",
        MndOptions {
            checks: vec!["case".into(), "condition".into()],
            ..MndOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        mnd(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages.iter().any(|m| m.contains("<argument>")),
        "disabled argument check should suppress call args: {messages:?}"
    );
}

#[test]
fn prealloc_respects_range_loops_off() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::PreallocOptions;

    let pkg = support::typecheck_fixture("prealloc", "example.com/prealloc", "bad.go");
    assert!(!support::run_analyzer(prealloc(), &pkg).is_empty());

    let mut bag = SettingsBag::new();
    bag.insert(
        "prealloc",
        PreallocOptions {
            range_loops: false,
            ..PreallocOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        prealloc(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "range-loops=false should suppress bad.go: {messages:?}"
    );
}

#[test]
fn tagalign_respects_align_off() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::TagalignOptions;

    let pkg = support::typecheck_fixture("tagalign", "example.com/tagalign", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "tagalign",
        TagalignOptions {
            align: false,
            sort: false,
            ..TagalignOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        tagalign(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "align=false sort=false should suppress bad.go: {messages:?}"
    );
}

#[test]
fn wsl_respects_allow_assign_and_anything() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::WslOptions;

    let pkg = support::typecheck_fixture("wsl", "example.com/wsl", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "wsl",
        WslOptions {
            allow_assign_and_anything: true,
            ..WslOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        wsl(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("assignments should only be cuddled")),
        "allow-assign-and-anything should suppress assign cuddling: {messages:?}"
    );
}

#[test]
fn unconvert_flags_identity_conversions() {
    let pkg = support::typecheck_fixture("unconvert", "example.com/unconvert", "bad.go");
    let messages = support::run_analyzer(unconvert(), &pkg);
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("unnecessary conversion"))
            .count()
            >= 2,
        "expected identity conversions on int and ID: {messages:?}"
    );
}

#[test]
fn unconvert_allows_real_conversions() {
    let pkg = support::typecheck_fixture("unconvert", "example.com/unconvert/ok", "ok.go");
    assert!(support::run_analyzer(unconvert(), &pkg).is_empty());
}

#[test]
fn unconvert_skips_float_by_default() {
    let pkg = support::typecheck_fixture("unconvert", "example.com/unconvert/fast", "fast_math.go");
    assert!(
        support::run_analyzer(unconvert(), &pkg).is_empty(),
        "float/complex identity conversions must stay when fast-math is off"
    );
}

#[test]
fn unconvert_fast_math_flags_float() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::UnconvertOptions;

    let pkg = support::typecheck_fixture("unconvert", "example.com/unconvert/fast", "fast_math.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "unconvert",
        UnconvertOptions {
            fast_math: true,
            ..UnconvertOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        unconvert(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("unnecessary conversion"))
            .count()
            >= 2,
        "fast-math should flag float/complex identity conversions: {messages:?}"
    );
}

#[test]
fn exhaustruct_flags_missing_fields() {
    let pkg = support::typecheck_fixture("exhaustruct", "example.com/exhaustruct", "bad.go");
    let messages = support::run_analyzer(exhaustruct(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("missing field Y")),
        "expected missing Y: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("missing fields") && m.contains("X") && m.contains("Y")),
        "expected missing X, Y on empty lit: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("<anonymous>") && m.contains("missing field B")),
        "expected anonymous missing B: {messages:?}"
    );
}

#[test]
fn exhaustruct_allows_complete_and_optional() {
    let pkg = support::typecheck_fixture("exhaustruct", "example.com/exhaustruct/ok", "ok.go");
    let messages = support::run_analyzer(exhaustruct(), &pkg);
    assert!(
        messages.is_empty(),
        "expected no diagnostics (optional Z + error return): {messages:?}"
    );
}

#[test]
fn exhaustruct_include_filters_types() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::ExhaustructOptions;

    let pkg = support::typecheck_fixture(
        "exhaustruct",
        "example.com/exhaustruct/include",
        "include.go",
    );
    let mut bag = SettingsBag::new();
    bag.insert(
        "exhaustruct",
        ExhaustructOptions {
            include: vec![r".*\.Included$".into()],
            ..ExhaustructOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        exhaustruct(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Included") && m.contains("missing")),
        "include should flag Included: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("Other")),
        "Other must be skipped by include filter: {messages:?}"
    );
}

#[test]
fn exhaustruct_allow_empty_declarations() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::ExhaustructOptions;

    let pkg = support::typecheck_fixture(
        "exhaustruct",
        "example.com/exhaustruct/emptydecl",
        "empty_decl.go",
    );
    assert!(
        !support::run_analyzer(exhaustruct(), &pkg).is_empty(),
        "empty decls must be flagged by default"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "exhaustruct",
        ExhaustructOptions {
            allow_empty_declarations: true,
            ..ExhaustructOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        exhaustruct(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "allow-empty-declarations should silence var/:= empties: {messages:?}"
    );
}

#[test]
fn exhaustive_flags_missing_cases() {
    let pkg = support::typecheck_fixture("exhaustive", "example.com/exhaustive", "bad.go");
    let messages = support::run_analyzer(exhaustive(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("missing cases") && m.contains("C")),
        "expected missing C: {messages:?}"
    );
}

#[test]
fn exhaustive_allows_complete_switch() {
    let pkg = support::typecheck_fixture("exhaustive", "example.com/exhaustive/ok", "ok.go");
    let messages = support::run_analyzer(exhaustive(), &pkg);
    assert!(
        messages.is_empty(),
        "expected no diagnostics for complete switches: {messages:?}"
    );
}

#[test]
fn exhaustive_default_signifies_exhaustive() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::ExhaustiveOptions;

    let pkg =
        support::typecheck_fixture("exhaustive", "example.com/exhaustive/def", "default_ok.go");
    // Default off: missing Green/Blue.
    let messages = support::run_analyzer(exhaustive(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("missing cases")),
        "default alone should not satisfy exhaustiveness: {messages:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "exhaustive",
        ExhaustiveOptions {
            default_signifies_exhaustive: true,
            ..ExhaustiveOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        exhaustive(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "default-signifies-exhaustive should silence: {messages:?}"
    );
}

#[test]
fn musttag_flags_missing_json_tags() {
    let pkg = support::typecheck_fixture("musttag", "example.com/musttag", "bad.go");
    let messages = support::run_analyzer(musttag(), &pkg);
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("annotated with the `json` tag"))
            .count()
            >= 2,
        "expected Marshal + Unmarshal diagnostics: {messages:?}"
    );
}

#[test]
fn musttag_allows_tagged_structs() {
    let pkg = support::typecheck_fixture("musttag", "example.com/musttag/ok", "ok.go");
    let messages = support::run_analyzer(musttag(), &pkg);
    assert!(
        messages.is_empty(),
        "expected no diagnostics for tagged structs: {messages:?}"
    );
}

#[test]
fn musttag_custom_functions_from_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::{MusttagFunc, MusttagOptions};

    let pkg = support::typecheck_fixture("musttag", "example.com/musttag", "custom.go");
    assert!(
        support::run_analyzer(musttag(), &pkg).is_empty(),
        "custom DecodeYAML is not a builtin"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "musttag",
        MusttagOptions {
            functions: vec![MusttagFunc {
                name: "example.com/musttag.DecodeYAML".into(),
                tag: "yaml".into(),
                arg_pos: 1,
            }],
        },
    );
    let messages = support::run_analyzer_with_settings(
        musttag(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| m.contains("`yaml` tag")),
        "custom function should require yaml tags: {messages:?}"
    );
}

#[test]
fn loggercheck_flags_odd_kv_pairs() {
    let pkg = support::typecheck_fixture("loggercheck", "example.com/loggercheck", "bad.go");
    let messages = support::run_analyzer(loggercheck(), &pkg);
    let odd = messages
        .iter()
        .filter(|m| m.contains("odd number of arguments"))
        .count();
    assert!(
        odd >= 5,
        "expected multiple odd-kv diagnostics, got {odd}: {messages:?}"
    );
}

#[test]
fn loggercheck_allows_even_kv_pairs() {
    let pkg = support::typecheck_fixture("loggercheck", "example.com/loggercheck/ok", "ok.go");
    let messages = support::run_analyzer(loggercheck(), &pkg);
    assert!(
        messages.is_empty(),
        "expected no diagnostics for even kv pairs: {messages:?}"
    );
}

#[test]
fn loggercheck_custom_rules_from_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::LoggercheckOptions;

    let pkg = support::typecheck_fixture("loggercheck", "example.com/loggercheck", "custom.go");
    assert!(
        support::run_analyzer(loggercheck(), &pkg).is_empty(),
        "MyLog is not a builtin logger"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "loggercheck",
        LoggercheckOptions {
            rules: vec!["example.com/loggercheck.MyLog".into()],
            ..LoggercheckOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        loggercheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("odd number of arguments"))
            .count()
            >= 2,
        "custom rule should flag odd kv: {messages:?}"
    );
}

#[test]
fn loggercheck_disable_slog_skips_diagnostics() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::LoggercheckOptions;

    let pkg = support::typecheck_fixture("loggercheck", "example.com/loggercheck", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "loggercheck",
        LoggercheckOptions {
            slog: false,
            ..LoggercheckOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        loggercheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "slog=false should skip slog calls: {messages:?}"
    );
}

#[test]
fn loggercheck_require_string_key_and_noprintflike() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::LoggercheckOptions;

    let pkg = support::typecheck_fixture("loggercheck", "example.com/loggercheck", "settings.go");
    assert!(
        support::run_analyzer(loggercheck(), &pkg).is_empty(),
        "defaults should not flag requirestringkey/noprintflike"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "loggercheck",
        LoggercheckOptions {
            require_string_key: true,
            no_printf_like: true,
            ..LoggercheckOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        loggercheck(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("inlined constant strings")),
        "require-string-key: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("format specifier")),
        "no-printf-like: {messages:?}"
    );
}

#[test]
fn sloglint_flags_mixed_args_by_default() {
    let pkg = support::typecheck_fixture("sloglint", "example.com/sloglint", "bad.go");
    let messages = support::run_analyzer(sloglint(), &pkg);
    let mixed = messages
        .iter()
        .filter(|m| m.contains("should not be mixed"))
        .count();
    assert!(
        mixed >= 3,
        "expected mixed-args diagnostics, got {mixed}: {messages:?}"
    );
}

#[test]
fn sloglint_allows_pure_kv_or_attrs() {
    let pkg = support::typecheck_fixture("sloglint", "example.com/sloglint/ok", "ok.go");
    let messages = support::run_analyzer(sloglint(), &pkg);
    assert!(
        messages.is_empty(),
        "expected no diagnostics for clean slog usage: {messages:?}"
    );
}

#[test]
fn sloglint_settings_static_msg_forbidden_keys_and_attr_only() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::SloglintOptions;

    let pkg = support::typecheck_fixture("sloglint", "example.com/sloglint", "settings.go");
    assert!(
        support::run_analyzer(sloglint(), &pkg).is_empty(),
        "defaults should only enforce no-mixed-args"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "sloglint",
        SloglintOptions {
            no_mixed_args: false,
            attr_only: true,
            static_msg: true,
            msg_style: Some("lowercased".into()),
            no_global: Some("default".into()),
            forbidden_keys: vec!["time".into(), "level".into()],
            no_raw_keys: true,
            allowed_keys: vec!["user_id".into()],
            ..SloglintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        sloglint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("default logger should not be used")),
        "no-global: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("string literal or a constant")),
        "static-msg: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("message should be lowercased")),
        "msg-style: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("forbidden") && m.contains("time")),
        "forbidden-keys: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("key-value pairs should not be used")),
        "attr-only: {messages:?}"
    );
}

#[test]
fn testifylint_flags_common_anti_patterns() {
    let pkg = support::typecheck_fixture("testifylint", "example.com/testifylint", "bad.go");
    let messages = support::run_analyzer(testifylint(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("bool-compare")),
        "bool-compare: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("compares")),
        "compares: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("empty")),
        "empty: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("error-nil")),
        "error-nil: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("nil-compare")),
        "nil-compare: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("len")),
        "len: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("float-compare")),
        "float-compare: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("zero")),
        "zero: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("negative-positive")),
        "negative-positive: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("useless-assert")),
        "useless-assert: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("contains")),
        "contains: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("equal-values")),
        "equal-values: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("regexp")),
        "regexp: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("error-is-as")),
        "error-is-as: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("encoded-compare")),
        "encoded-compare: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("expected-actual")),
        "expected-actual: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("time-compare")),
        "time-compare: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("formatter")),
        "formatter: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("suite-extra-assert-call")),
        "suite-extra-assert-call: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("suite-dont-use-pkg")),
        "suite-dont-use-pkg: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("suite-subtest-run")),
        "suite-subtest-run: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("suite-method-signature")),
        "suite-method-signature: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("suite-broken-parallel")),
        "suite-broken-parallel: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("require-error")),
        "require-error: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("go-require") && m.contains("require must only")),
        "go-require goroutine: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("go-require") && m.contains("FailNow")),
        "go-require FailNow: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("go-require") && m.contains("helperWithRequire")),
        "go-require nested helper: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("go-require") && m.contains("http handlers")),
        "go-require http handler: {messages:?}"
    );
    assert!(
        messages.iter().all(|m| !m.contains("suite-thelper")),
        "suite-thelper should be off by default: {messages:?}"
    );
}

#[test]
fn testifylint_allows_idiomatic_assertions() {
    let pkg = support::typecheck_fixture("testifylint", "example.com/testifylint/ok", "ok.go");
    let messages = support::run_analyzer(testifylint(), &pkg);
    assert!(
        messages.is_empty(),
        "expected no diagnostics for idiomatic testify usage: {messages:?}"
    );
}

#[test]
fn testifylint_flags_blank_imports() {
    let pkg =
        support::typecheck_fixture("testifylint", "example.com/testifylint/blank", "blank.go");
    let messages = support::run_analyzer(testifylint(), &pkg);
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("blank-import"))
            .count()
            >= 2,
        "blank-import: {messages:?}"
    );
}

#[test]
fn testifylint_flags_mock_expect() {
    let pkg = support::typecheck_fixture(
        "testifylint",
        "example.com/testifylint/mockexpect",
        "mock_expect.go",
    );
    let messages = support::run_analyzer(testifylint(), &pkg);
    let mock_msgs: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("mock-expect"))
        .collect();
    assert!(
        mock_msgs
            .iter()
            .any(|m| m.contains("u.EXPECT().CreateUser")),
        "CreateUser: {mock_msgs:?}"
    );
    assert!(
        mock_msgs.iter().any(|m| m.contains("u.EXPECT().Void")),
        "Void: {mock_msgs:?}"
    );
    assert!(
        mock_msgs
            .iter()
            .any(|m| m.contains("u.EXPECT().CountUsers")),
        "CountUsers: {mock_msgs:?}"
    );
    assert!(
        mock_msgs.iter().any(|m| m.contains("u.EXPECT().Variadic")),
        "Variadic: {mock_msgs:?}"
    );
    assert!(
        mock_msgs
            .iter()
            .any(|m| m.contains("holder.user.EXPECT().Void")),
        "holder.user: {mock_msgs:?}"
    );
    assert!(
        mock_msgs
            .iter()
            .any(|m| m.contains("mockFrom(u).EXPECT().Void")),
        "mockFrom: {mock_msgs:?}"
    );
    // Ignored cases must not report.
    assert!(
        mock_msgs.iter().all(|m| !m.contains("DoesNotExist")),
        "ignored DoesNotExist: {mock_msgs:?}"
    );
    assert!(
        mock_msgs.len() >= 10,
        "expected many mock-expect hits, got {}: {mock_msgs:?}",
        mock_msgs.len()
    );
}

#[test]
fn testifylint_disable_all_then_enable_subset() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::TestifylintOptions;

    let pkg = support::typecheck_fixture("testifylint", "example.com/testifylint", "settings.go");
    let all = support::run_analyzer(testifylint(), &pkg);
    assert!(
        all.iter().any(|m| m.contains("bool-compare")),
        "defaults should flag bool-compare: {all:?}"
    );
    assert!(
        all.iter().any(|m| m.contains("empty")),
        "defaults should flag empty: {all:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "testifylint",
        TestifylintOptions {
            disable_all: true,
            enable: vec!["bool-compare".into()],
            ..TestifylintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        testifylint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| m.contains("bool-compare")),
        "enabled bool-compare: {messages:?}"
    );
    assert!(
        messages.iter().all(|m| !m.contains("empty:")),
        "empty should be disabled: {messages:?}"
    );
}

#[test]
fn testifylint_suite_thelper_when_enabled() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::TestifylintOptions;

    let pkg = support::typecheck_fixture("testifylint", "example.com/testifylint", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "testifylint",
        TestifylintOptions {
            disable_all: true,
            enable: vec!["suite-thelper".into()],
            ..TestifylintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        testifylint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("suite-thelper") && m.contains("s.T().Helper()")),
        "suite-thelper: {messages:?}"
    );
}

#[test]
fn testifylint_require_error_fn_pattern() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::TestifylintOptions;

    let pkg = support::typecheck_fixture("testifylint", "example.com/testifylint", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "testifylint",
        TestifylintOptions {
            disable_all: true,
            enable: vec!["require-error".into()],
            require_error_fn_pattern: Some("^NoError$".into()),
            ..TestifylintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        testifylint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| m.contains("require-error")),
        "fn-pattern NoError should still flag: {messages:?}"
    );

    let mut bag_all = SettingsBag::new();
    bag_all.insert(
        "testifylint",
        TestifylintOptions {
            disable_all: true,
            enable: vec!["require-error".into()],
            require_error_fn_pattern: Some("^DoesNotMatch$".into()),
            ..TestifylintOptions::default()
        },
    );
    let none = support::run_analyzer_with_settings(
        testifylint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag_all),
            ..RunnerOptions::default()
        },
    );
    assert!(
        none.iter().all(|m| !m.contains("require-error")),
        "non-matching fn-pattern should suppress: {none:?}"
    );
}

#[test]
fn testifylint_go_require_ignore_http_handlers() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::TestifylintOptions;

    let pkg = support::typecheck_fixture("testifylint", "example.com/testifylint", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "testifylint",
        TestifylintOptions {
            disable_all: true,
            enable: vec!["go-require".into()],
            go_require_ignore_http_handlers: true,
            ..TestifylintOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        testifylint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("go-require") && m.contains("require must only")),
        "goroutine require still flagged: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .all(|m| !(m.contains("go-require") && m.contains("http handlers"))),
        "http handlers should be ignored: {messages:?}"
    );
}

#[test]
fn exptostd_flags_exp_maps() {
    let pkg = support::typecheck_fixture("exptostd", "example.com/exptostd", "bad_maps.go");
    let messages = support::run_analyzer(exptostd(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("golang.org/x/exp/maps.Clone()") && m.contains("maps.Clone()")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("golang.org/x/exp/maps.Clear()") && m.contains("clear()")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m
                .contains("Import statement 'golang.org/x/exp/maps' may be replaced by 'maps'")),
        "{messages:?}"
    );
}

#[test]
fn exptostd_flags_exp_slices_import_only_when_fully_replaceable() {
    let pkg = support::typecheck_fixture("exptostd", "example.com/exptostd", "bad_slices.go");
    let messages = support::run_analyzer(exptostd(), &pkg);
    assert!(
        messages.iter().any(|m| m
            .contains("Import statement 'golang.org/x/exp/slices' may be replaced by 'slices'")),
        "{messages:?}"
    );
    // Upstream reports only the import when every slices call is 1:1 replaceable.
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("golang.org/x/exp/slices.Equal()")),
        "per-call slices diagnostics should be omitted: {messages:?}"
    );
}

#[test]
fn exptostd_flags_exp_constraints() {
    let pkg = support::typecheck_fixture("exptostd", "example.com/exptostd", "bad_constraints.go");
    let messages = support::run_analyzer(exptostd(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m
                .contains("golang.org/x/exp/constraints.Ordered can be replaced by cmp.Ordered")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m
            .contains("Import statement 'golang.org/x/exp/constraints' may be replaced by 'cmp'")),
        "{messages:?}"
    );
}

#[test]
fn exptostd_allows_non_exp_maps() {
    let pkg = support::typecheck_fixture("exptostd", "example.com/exptostd/ok", "ok.go");
    assert!(support::run_analyzer(exptostd(), &pkg).is_empty());
}

#[test]
fn modernize_flags_common_patterns() {
    let pkg = support::typecheck_fixture("modernize", "example.com/modernize", "bad.go");
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("interface{} can be replaced by any")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("for loop can be modernized using range")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("if/else statement can be modernized using min")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("fmt.Appendf") || m.contains("Appendf")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("sort.Slice can be modernized using slices.Sort")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("copying variable is unneeded")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("HasPrefix + TrimPrefix can be simplified to CutPrefix")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("loop can be modernized using slices.Contains")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_stringsseq() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/stringsseq",
        "stringsseq.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("SplitSeq") || m.contains("FieldsSeq")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_waitgroupgo() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/waitgroupgo",
        "waitgroupgo.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("WaitGroup.Go")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_mapsloop() {
    let pkg =
        support::typecheck_fixture("modernize", "example.com/modernize/mapsloop", "mapsloop.go");
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Replace m[k]=v loop with maps.Copy")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_slicesbackward() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/slicesbackward",
        "slicesbackward.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("slices.Backward")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_reflecttypefor() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/reflecttypefor",
        "reflecttypefor.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages.iter().filter(|m| m.contains("TypeFor")).count() >= 2,
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_reflecttypeassert() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/reflecttypeassert",
        "reflecttypeassert.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let hits: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("can be simplified using reflect.TypeAssert"))
        .collect();
    assert_eq!(
        hits.len(),
        6,
        "expected exactly 6 reflecttypeassert hits (6 positive / negatives skipped), got {} {messages:?}",
        hits.len()
    );
    assert!(
        hits.iter().any(|m| m.contains("Interface().(string)")),
        "{messages:?}"
    );
    assert!(
        hits.iter().any(|m| m.contains("Interface().(payload)")),
        "{messages:?}"
    );
    assert!(
        hits.iter().any(|m| m.contains("Interface().(io.Reader)")),
        "{messages:?}"
    );
    assert!(
        hits.iter().any(|m| m.contains("Interface().(int)")),
        "{messages:?}"
    );
    assert!(
        hits.iter().any(|m| m.contains("Interface().(error)")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_atomictypes() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/atomictypes",
        "atomictypes.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let hits: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("may be simplified using atomic."))
        .collect();
    // goodLocal (x Int32), goodShadowAlias (x Int32), goodField (X.x Int32 + Z.y Int64)
    assert_eq!(
        hits.len(),
        4,
        "expected exactly 4 atomictypes hits, got {} {messages:?}",
        hits.len()
    );
    assert_eq!(
        hits.iter().filter(|m| m.contains("atomic.Int32")).count(),
        3,
        "{messages:?}"
    );
    assert!(
        hits.iter().any(|m| m.contains("atomic.Int64")),
        "{messages:?}"
    );
    // Negatives must not be reported.
    assert!(
        !messages.iter().any(|m| m.contains("var x2 ")),
        "init-assigned var must be skipped: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("var z ")),
        "unsynchronized load must be skipped: {messages:?}"
    );
}

#[test]
fn modernize_flags_testingcontext() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/testingcontext",
        "testingcontext.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("t.Context")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_unsafefuncs() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/unsafefuncs",
        "unsafefuncs.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages.iter().filter(|m| m.contains("unsafe.Add")).count() >= 2,
        "{messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("namedUP")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_importcomment() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/importcomment",
        "importcomment.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("canonical import path comment")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_stringscut() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/stringscut",
        "stringscut.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("strings.Cut"))
            .count()
            >= 2,
        "{messages:?}"
    );
    assert!(
        messages.iter().filter(|m| m.contains("bytes.Cut")).count() >= 2,
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_newexpr() {
    let pkg =
        support::typecheck_fixture("modernize", "example.com/modernize/newexpr", "newexpr.go");
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("intVar can be an inlinable wrapper around new(expr)")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("varOf can be an inlinable wrapper around new(expr)")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("alreadyAnnotated can be an inlinable wrapper around new(expr)")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("call of intVar(x) can be simplified to new(x)")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("call of stringVar(x) can be simplified to new(x)")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("call of varOf(x) can be simplified to new(x)")),
        "{messages:?}"
    );
    // Untyped int → int64 parameter must not rewrite.
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("call of int64Var(x) can be simplified to new(x)")),
        "{messages:?}"
    );
    // Variadic must not be flagged as a new-like wrapper.
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("variadic can be an inlinable wrapper")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_errorsastype() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/errorsastype",
        "errorsastype.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let as_type: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("errors.As can be simplified using AsType"))
        .collect();
    assert!(
        as_type.len() >= 9,
        "expected >=9 AsType suggestions, got {} {messages:?}",
        as_type.len()
    );
    assert!(
        as_type.iter().any(|m| m.contains("AsType[*os.PathError]")),
        "{messages:?}"
    );
    assert!(
        as_type.iter().any(|m| m.contains("AsType[*os.LinkError]")),
        "{messages:?}"
    );
    assert!(
        as_type.iter().any(|m| m.contains("AsType[FooError]")),
        "{messages:?}"
    );
}

#[test]
fn modernize_flags_stringsbuilder() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/stringsbuilder",
        "stringsbuilder.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let hits: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("using string += string in a loop is inefficient"))
        .collect();
    assert_eq!(
        hits.len(),
        4,
        "expected exactly 4 stringsbuilder hits (4 positive / 5 negative), got {} {messages:?}",
        hits.len()
    );
}

#[test]
fn modernize_flags_slicesdelete() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/slicesdelete",
        "slicesdelete.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let hits: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("Replace append with slices.Delete"))
        .collect();
    assert_eq!(
        hits.len(),
        8,
        "expected exactly 8 slicesdelete hits, got {} {messages:?}",
        hits.len()
    );
}

#[test]
fn modernize_flags_bloop() {
    let pkg = support::typecheck_fixture("modernize", "example.com/modernize/bloop", "bloop.go");
    let messages = support::run_analyzer(modernize(), &pkg);
    let hits: Vec<_> = messages
        .iter()
        .filter(|m| m.contains("b.N can be modernized using b.Loop()"))
        .collect();
    assert_eq!(
        hits.len(),
        4,
        "expected exactly 4 bloop hits (A/C/D/E), got {} {messages:?}",
        hits.len()
    );
}

#[test]
fn modernize_flags_stditerators() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/stditerators",
        "stditerators.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    let struct_hits = messages
        .iter()
        .filter(|m| m.contains("NumFields/Field loop can simplified using Struct.Fields iteration"))
        .count();
    assert_eq!(
        struct_hits, 2,
        "expected 2 Struct hits (C-style + range), got {struct_hits}: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Len/At loop can simplified using Tuple.Variables iteration")),
        "expected a Tuple hit, got {messages:?}"
    );
    // extraUse / plainSlice must not be flagged.
    let total = messages
        .iter()
        .filter(|m| m.contains("loop can simplified using"))
        .count();
    assert_eq!(
        total, 3,
        "expected exactly 3 stditerators hits, got {total}: {messages:?}"
    );
}

#[test]
fn modernize_flags_slicescontains_variants() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/slicescontains",
        "slicescontains.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("slices.Contains") && !m.contains("ContainsFunc"))
            .count()
            >= 4,
        "expected Contains variants, got {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("slices.ContainsFunc")),
        "expected ContainsFunc, got {messages:?}"
    );
}

#[test]
fn modernize_flags_stringscutprefix_pattern2_and_bytes() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/stringscutprefix",
        "stringscutprefix.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("TrimPrefix can be simplified to CutPrefix")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("TrimSuffix can be simplified to CutSuffix")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("HasPrefix + TrimPrefix can be simplified to CutPrefix"))
            .count()
            >= 1,
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("HasSuffix + TrimSuffix can be simplified to CutSuffix")),
        "{messages:?}"
    );
}

#[test]
fn modernize_allows_modern_code() {
    let pkg = support::typecheck_fixture("modernize", "example.com/modernize/ok", "ok.go");
    assert!(
        support::run_analyzer(modernize(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(modernize(), &pkg)
    );
}

#[test]
fn modernize_flags_obsolete_plusbuild() {
    let pkg = support::typecheck_fixture(
        "modernize",
        "example.com/modernize/plusbuild",
        "plusbuild.go",
    );
    let messages = support::run_analyzer(modernize(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("+build line is no longer needed")),
        "{messages:?}"
    );
}

#[test]
fn modernize_disable_skips_checkers() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::ModernizeOptions;

    let pkg = support::typecheck_fixture("modernize", "example.com/modernize", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "modernize",
        ModernizeOptions {
            disable: vec!["any".into(), "rangeint".into(), "minmax".into()],
        },
    );
    let messages = support::run_analyzer_with_settings(
        modernize(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("interface{} can be replaced by any")),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("for loop can be modernized using range")),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("if/else statement can be modernized using min")),
        "{messages:?}"
    );
}

#[test]
fn gocritic_flags_common_patterns() {
    let pkg = support::typecheck_fixture("gocritic", "example.com/gocritic", "bad.go");
    let messages = support::run_analyzer(gocritic(), &pkg);
    let expect = [
        "else if cond",
        "rewrite switch statement to if",
        "default` case as first or as last",
        "switch true",
        "always true",
        "always false",
        "can be len(",
        "could simplify",
        "*new(bool)",
        "append result not assigned",
        "is duplicated",
        "should not be capitalized",
        "will exit",
        "rewrite if-else to switch",
        "can re-write as",
        "flag.BoolVar",
        "no-op append",
        "suspicious Join",
        "probably meant -1",
        "x++",
        "x *=",
        "duplicated args",
        "both branches in if statement have same body",
        "identical LHS and RHS",
        "contains whitespace",
        "suspicious whitespace",
        "always panics",
        "type switch with assignment",
        "in loop; probably meant",
        "condition is suspicious",
        "replace `",
        "MustCompile",
        "WaitGroup.Done",
        "strings.Split method",
        "arguments order looks reversed",
        "must go before the",
        "Code generated .* DO NOT EDIT",
        "put a space between",
        "Deprecated: ` (note the casing)",
        "from/to types are identical",
    ];
    for needle in expect {
        assert!(
            messages.iter().any(|m| m.contains(needle)),
            "missing `{needle}` in {messages:?}"
        );
    }
}

#[test]
fn gocritic_allows_clean_code() {
    let pkg = support::typecheck_fixture("gocritic", "example.com/gocritic/ok", "ok.go");
    let messages = support::run_analyzer(gocritic(), &pkg);
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:?}");
}

#[test]
fn gocritic_disabled_checks_are_skipped() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GocriticOptions;

    let pkg = support::typecheck_fixture("gocritic", "example.com/gocritic", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "gocritic",
        GocriticOptions {
            enable_all: true,
            disabled_checks: vec![
                "appendAssign".into(),
                "ifElseChain".into(),
                "underef".into(),
            ],
            ..GocriticOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        gocritic(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("append result not assigned")),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("rewrite if-else to switch")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("else if cond")),
        "{messages:?}"
    );
}

#[test]
fn gocritic_enable_all_extras() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_style::GocriticOptions;

    let pkg = support::typecheck_fixture("gocritic", "example.com/gocritic/extras", "extras.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "gocritic",
        GocriticOptions {
            enable_all: true,
            ..GocriticOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        gocritic(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    let expect = [
        "replace `len(s)",
        "replace empty case containing only fallthrough",
        "empty var() block",
        "empty const() block",
        "empty type() block",
        "use new octal literal style, 0o755",
        "returned expr is always nil",
        "consider to change order in expression to p == nil",
        "consider to change order in expression to *p == 10",
        "can rewrite as `defer fmt.Println",
        "consider to move `sideEffectExtra()` before if",
        "shadowing of predeclared identifier: len",
        "shadowing of predeclared identifier: complex64",
        "package is imported 2 times under different aliases",
        "remove commented-out \"os\" import",
        "func(a int, b int) could be replaced with func(a, b int)",
        "\"dir/\" contains a path separator",
        "append all `ns` data while range it",
        "nil check may not be enough, check for len",
        "function argument `withWidth(w)` is duplicated",
        "consider to change `methodFoo.bar` to `f.bar`",
        "copy of xs (512 bytes) can be avoided with &xs",
        "'.com' should probably be '\\.com'",
        "^ applied only to",
        "is duplicated",
        "`\\w` intersects with `_`",
        "cmp func must use xs slice in comparison",
        "use db.Exec() if returned result is not needed",
        "ignoring Query() rows result may lead to a connection leak",
        "rewrite if-else to type switch statement",
        "truncation in comparison",
        "definition of type 'typeDefFirstRecv'",
        "Possible resource leak, 'defer' is called in the 'for' loop",
        "prefer 0x over 0X",
        "don't mix hex literal letter digits casing",
        "invert if cond, replace body with `continue`",
        "may want to add detail/assignee to this TODO/FIXME/BUG comment",
        "silencing go lint doc-comment warnings is unadvised",
        "block doesn't have definitions, can be simply deleted",
        "re-assignment to `err` can be replaced with",
        "http.NoBody should be preferred",
        "utf8.DecodeRuneInString",
        "consider writing single byte rune '\\n' with w.WriteByte('\\n')",
        "bytes.Index(",
        "can simplify `[]byte($s)` to `$s`",
        "can replace `string($b) == \"\"`",
        "can replace `len(string($b))`",
        "filepath.Join(",
        "can replace `strings.Compare`",
        "avoid bytes.Repeat",
        "suspicious sort.StringSlice usage",
        "rewrite as for-range so compiler can recognize",
        "fmt.Fprint(w, ...) should be preferred",
        "fmt.Fprintf(w, ...) should be preferred",
        "fmt.Fprintln(w, ...) should be preferred",
        "w.WriteString(s) should be preferred",
        "use m.LoadAndDelete to perform load+delete",
        "use errors.New(msg) or fmt.Errorf",
        "use errors.New(f(...)) or fmt.Errorf",
        "can simplify `strings.Join` to `x + y`",
        "can simplify `strings.Join` to `x + y + z`",
        "can simplify `strings.Join` to `x + glue + y`",
        "sync.OnceFunc(f) result is not used",
        "consider to assign sync.OnceFunc(f) to a variable",
        "consider replacing with strings.EqualFold(x, y)",
        "consider replacing with !strings.EqualFold(x, y)",
        "consider replacing with bytes.EqualFold(xb, yb)",
        "use %q instead of \"%s\" for quoted strings",
        "use %#q instead of \"`%s`\" for backquoted strings",
        "use t.UnixMilli() instead of",
        "use tp.UnixMicro() instead of",
        "can combine chain of 2 appends into one",
        "defer appendCombineExtra() is placed just before return",
        "s is already string",
        "use w.String() instead",
        "could simplify [](func()) to []func()",
        "shadow of imported package 'filepath'",
        "consider giving a name to these results",
        "include an explanation for nolint directive",
        "is heavy (",
        "each iteration copies",
        "consider `m' to be of non-pointer type",
        "consider `ch' to be of non-pointer type",
        "function has more than 5 results",
        "may want to evaluate evalOrderMutate(&x) before the return statement",
        "label label1 is redundant",
        "change `continue outer` to `break`",
        "Possibly return is missed after the http.Error call",
        "may want to remove commented-out code",
        "don't embed sync.Mutex",
        "don't embed *sync.RWMutex",
        "defer is missing, mutex is unlocked immediately",
        "suspicious unlock, maybe Unlock was intended?",
        "suspicious unlock, maybe RUnlock was intended?",
        "maybe defer rw.Unlock() was intended?",
        "maybe defer rw.RUnlock() was intended?",
        "suspicious reassignment of error from another package",
        "err error is unchecked, maybe intended to check it instead of err2",
        "can simplify `!!x` to `x`",
        "can simplify `!(a >= b)` to `a < b`",
        "can simplify `!x == !y` to `x == y`",
        "can simplify `a > b || a == b` to `a >= b`",
        "can simplify `a < b + 1` to `a <= b`",
        "can simplify `a + 1 > b` to `a >= b`",
        "can simplify `a >= b + 1` to `a > b`",
        "can simplify `!(a >= b + 1)` to `a <= b`",
        "can simplify `a > 10 && a < 12` to `a == 11`",
        "can simplify `a < 11 || a > 11` to `a != 11`",
        "can re-write `[0-9]+` as `\\d+`",
        "can re-write `(?:a|b|c)` as `[abc]`",
        "can re-write `foo|fo` as `foo?`",
        "can re-write `axx*y` as `ax+y`",
    ];
    for needle in expect {
        assert!(
            messages.iter().any(|m| m.contains(needle)),
            "missing `{needle}` in {messages:?}"
        );
    }
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("consider writing single byte rune"))
            .count(),
        1,
        "{messages:?}"
    );
}

#[test]
fn gocritic_extras_off_by_default() {
    let pkg = support::typecheck_fixture("gocritic", "example.com/gocritic/extras", "extras.go");
    let messages = support::run_analyzer(gocritic(), &pkg);
    assert!(
        !messages.iter().any(|m| {
            m.contains("empty var() block")
                || m.contains("octal literal")
                || m.contains("yoda")
                || m.contains("always nil")
                || m.contains("can rewrite as `defer")
                || m.contains("before if")
                || m.contains("shadowing of predeclared")
                || m.contains("imported 2 times")
                || m.contains("commented-out")
                || m.contains("could be replaced with func(a, b int)")
                || m.contains("path separator")
                || m.contains("append all")
                || m.contains("nil check may not be enough")
                || m.contains("is duplicated")
                || m.contains("consider to change `methodFoo")
                || m.contains("can be avoided with &")
                || m.contains("should probably be")
                || m.contains("applied only to")
                || m.contains("intersects with")
                || m.contains("must use xs slice")
                || m.contains("use db.Exec()")
                || m.contains("connection leak")
                || m.contains("type switch statement")
                || m.contains("truncation in comparison")
                || m.contains("definition of type")
                || m.contains("Possible resource leak")
                || m.contains("prefer 0x over 0X")
                || m.contains("don't mix hex literal")
                || m.contains("invert if cond")
                || m.contains("detail/assignee")
                || m.contains("doc-comment warnings")
                || m.contains("block doesn't have definitions")
                || m.contains("re-assignment to")
                || m.contains("http.NoBody")
                || m.contains("DecodeRuneInString")
                || m.contains("bytes.Index(")
                || m.contains("can simplify `[]byte")
                || m.contains("filepath.Join(")
                || m.contains("strings.Compare")
                || m.contains("bytes.Repeat")
                || m.contains("sort.StringSlice")
                || m.contains("for-range so compiler")
                || m.contains("should be preferred")
                || m.contains("LoadAndDelete")
                || m.contains("errors.New")
                || m.contains("strings.Join")
                || m.contains("sync.OnceFunc")
                || m.contains("EqualFold")
                || m.contains("EqualFold")
                || m.contains("%q instead")
                || m.contains("%#q instead")
                || m.contains("UnixMilli")
                || m.contains("UnixMicro")
                || m.contains("combine chain of")
                || m.contains("just before return")
                || m.contains("already string")
                || m.contains(".String() instead")
                || m.contains("could simplify [](func())")
                || m.contains("shadow of imported")
                || m.contains("giving a name to these results")
                || m.contains("explanation for nolint")
                || m.contains("is heavy (")
                || m.contains("each iteration copies")
                || m.contains("non-pointer type")
                || m.contains("more than 5 results")
                || m.contains("before the return statement")
                || m.contains("label label1 is redundant")
                || m.contains("continue outer")
                || m.contains("return is missed after the http.Error")
                || m.contains("don't embed sync.Mutex")
                || m.contains("don't embed *sync.RWMutex")
        }),
        "extras should be off by default: {messages:?}"
    );
}
