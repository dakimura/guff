mod support;

use guff_comment::{dupword, godoclint, godot, godox};

#[test]
fn godot_flags_missing_periods() {
    let pkg = support::typecheck_fixture("godot", "example.com/godot", "bad.go");
    let messages = support::run_analyzer(godot(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("period")),
        "{messages:?}"
    );
}

#[test]
fn godot_allows_periods() {
    let pkg = support::typecheck_fixture("godot", "example.com/godot/ok", "ok.go");
    assert!(support::run_analyzer(godot(), &pkg).is_empty());
}

#[test]
fn godot_capital_and_exclude_respect_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_comment::GodotOptions;
    use guff_runner::RunnerOptions;

    let pkg = support::typecheck_fixture("godot", "example.com/godot/capital", "capital.go");

    // Defaults: period on, capital off → period issues only (FIXME line).
    let default_msgs = support::run_analyzer(godot(), &pkg);
    assert!(
        default_msgs.iter().any(|m| m.contains("period")),
        "default should flag missing period: {default_msgs:?}"
    );
    assert!(
        default_msgs.iter().all(|m| !m.contains("capital")),
        "default capital=false: {default_msgs:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "godot",
        GodotOptions {
            scope: "declarations".into(),
            exclude: vec!["^FIXME:".into()],
            period: false,
            capital: true,
        },
    );
    let messages = support::run_analyzer_with_settings(
        godot(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| m.contains("capital")),
        "capital=true should flag lowercase sentence start: {messages:?}"
    );
    assert!(
        messages.iter().all(|m| !m.contains("period")),
        "period=false should skip period: {messages:?}"
    );
}

#[test]
fn godox_flags_todo_and_fixme() {
    let pkg = support::typecheck_fixture("godox", "example.com/godox", "bad.go");
    let messages = support::run_analyzer(godox(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("TODO")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("FIXME")),
        "{messages:?}"
    );
}

#[test]
fn godox_allows_non_keyword_comments() {
    let pkg = support::typecheck_fixture("godox", "example.com/godox/ok", "ok.go");
    assert!(support::run_analyzer(godox(), &pkg).is_empty());
}

#[test]
fn godox_custom_keywords_respect_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_comment::GodoxOptions;
    use guff_runner::RunnerOptions;

    let pkg = support::typecheck_fixture("godox", "example.com/godox/custom", "custom.go");
    assert!(
        support::run_analyzer(godox(), &pkg).is_empty(),
        "defaults should ignore NOTE/HACK: {:?}",
        support::run_analyzer(godox(), &pkg)
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "godox",
        GodoxOptions {
            keywords: vec!["NOTE".into(), "HACK".into()],
        },
    );
    let messages = support::run_analyzer_with_settings(
        godox(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.iter().any(|m| m.contains("NOTE")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("HACK")),
        "{messages:?}"
    );
    assert_eq!(messages.len(), 2, "{messages:?}");
}

#[test]
fn dupword_flags_duplicates_in_comments_and_strings() {
    let pkg = support::typecheck_fixture("dupword", "example.com/dupword", "bad.go");
    let messages = support::run_analyzer(dupword(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Duplicate words") && m.contains("is")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Duplicate words") && m.contains("the")),
        "{messages:?}"
    );
}

#[test]
fn dupword_allows_clean_text() {
    let pkg = support::typecheck_fixture("dupword", "example.com/dupword/ok", "ok.go");
    assert!(support::run_analyzer(dupword(), &pkg).is_empty());
}

#[test]
fn dupword_keywords_ignore_comments_only_respect_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_comment::DupwordOptions;
    use guff_runner::RunnerOptions;

    let pkg =
        support::typecheck_fixture("dupword", "example.com/dupword/keywords", "keywords.go");

    let default_msgs = support::run_analyzer(dupword(), &pkg);
    assert!(
        default_msgs
            .iter()
            .any(|m| m.contains("the") && m.contains("Duplicate")),
        "{default_msgs:?}"
    );
    assert!(
        default_msgs
            .iter()
            .any(|m| m.contains("is") && m.contains("Duplicate")),
        "{default_msgs:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "dupword",
        DupwordOptions {
            keywords: vec!["the".into()],
            ignore: vec!["is".into()],
            comments_only: true,
        },
    );
    let messages = support::run_analyzer_with_settings(
        dupword(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Duplicate words") && m.contains("the")),
        "keywords=[the] should still flag comment: {messages:?}"
    );
    assert!(
        messages.iter().all(|m| !m.contains("(is)")),
        "ignore=[is] should skip is is: {messages:?}"
    );
    // comments-only: string "the the in string" must not produce a second hit
    // beyond the comment. With keywords filter both comment and string have
    // "the the"; comments-only should leave only the comment diagnostic.
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("Duplicate words") && m.contains("the"))
            .count(),
        1,
        "comments-only should skip string literal: {messages:?}"
    );
}

#[test]
fn godoclint_flags_pkg_doc_start_with_name_and_deprecated() {
    let pkg = support::typecheck_fixture("godoclint", "example.com/godoclint", "bad.go");
    let messages = support::run_analyzer(godoclint(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("package godoc should start with")),
        "pkg-doc: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("godoc should start with symbol name") && m.contains("Foo")),
        "start-with-name: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("deprecation note should be formatted")),
        "deprecated: {messages:?}"
    );
}

#[test]
fn godoclint_allows_well_formed_docs() {
    let pkg = support::typecheck_fixture("godoclint", "example.com/godoclint/ok", "ok.go");
    assert!(
        support::run_analyzer(godoclint(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(godoclint(), &pkg)
    );
}

#[test]
fn godoclint_flags_multiple_package_docs() {
    let pkg = support::typecheck_fixture_dir("godoclint/multi", "example.com/godoclint/multi");
    let messages = support::run_analyzer(godoclint(), &pkg);
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("package has more than one godoc"))
            .count()
            >= 2,
        "single-pkg-doc: {messages:?}"
    );
}

#[test]
fn godoclint_respects_enable_disable_settings() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_comment::GodoclintOptions;
    use guff_runner::RunnerOptions;

    let pkg = support::typecheck_fixture("godoclint", "example.com/godoclint/settings", "bad.go");

    let mut bag = SettingsBag::new();
    bag.insert(
        "godoclint",
        GodoclintOptions {
            default: "none".into(),
            enable: vec!["pkg-doc".into()],
            disable: Vec::new(),
        },
    );
    let messages = support::run_analyzer_with_settings(
        godoclint(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("package godoc should start with")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .all(|m| !m.contains("godoc should start with symbol name")),
        "start-with-name disabled: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .all(|m| !m.contains("deprecation note")),
        "deprecated disabled: {messages:?}"
    );
}

/// godot's `declarations` scope is `getBlockComments() ++
/// getDeclarationComments()`, and guff had only the second half — a documented
/// spec inside a `var (` or `const (` group was never looked at.
///
/// The column rule is the part worth pinning, because it is upstream's and not
/// something the shape suggests: a comment inside a block counts only at column
/// **exactly 2**. The block itself is top level, so only its immediate contents
/// are; one nested a level deeper is deliberately skipped. The fixture holds
/// that case, a free-floating comment owned by no spec (which does count), and
/// a single-line `const` with no `Lparen` to be inside of.
#[test]
fn godot_checks_comments_inside_top_level_blocks() {
    let pkg = support::typecheck_fixture("godot", "example.com/godot", "bad.go");
    let found = support::run_analyzer(godot(), &pkg);
    // Every godot message is the same string, so the count is all a
    // message-level test can say: three top-level decl docs, the two block
    // comments the old code never looked at, and three multi-line comments
    // whose *line* was wrong rather than missing. `compat/golden/cases/godot`
    // pins which eight, line and column, against golangci-lint — and the three
    // multi-line ones are there because a fixture of single-line comments
    // passed this check while both position bugs were live.
    assert_eq!(found.len(), 8, "{found:?}");
}
