mod support;

use guff_comment::{dupword, godot, godox};

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
