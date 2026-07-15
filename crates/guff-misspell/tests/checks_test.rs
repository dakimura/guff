mod support;

use guff_analysis::validate;
use guff_analysis::SettingsBag;
use guff_misspell::{misspell, Options};
use guff_runner::{run_on_packages, RunnerOptions};

#[test]
fn misspell_flags_common_typos() {
    let pkg = support::typecheck_fixture("misspell", "example.com/misspell", "bad.go");
    let messages = support::run_analyzer(misspell(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("misspelling") && m.contains("Amercia")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("misspelling") && m.contains("brocoli")),
        "{messages:?}"
    );
}

#[test]
fn misspell_allows_correct_spelling() {
    let pkg = support::typecheck_fixture("misspell", "example.com/misspell/ok", "ok.go");
    assert!(support::run_analyzer(misspell(), &pkg).is_empty());
}

#[test]
fn misspell_analyzer_graph_is_valid() {
    validate(&[misspell()]).expect("valid analyzer graph");
}

#[test]
fn misspell_restricted_mode_skips_string_literals() {
    let pkg = support::typecheck_fixture("misspell", "example.com/misspell", "bad.go");
    let mut bag = SettingsBag::new();
    bag.insert(
        "misspell",
        Options {
            locale: "US".into(),
            mode: "restricted".into(),
            ..Options::default()
        },
    );
    let result = run_on_packages(
        &[misspell()],
        std::slice::from_ref(&pkg),
        &RunnerOptions {
            sequential: true,
            settings: std::sync::Arc::new(bag),
            ..RunnerOptions::default()
        },
    )
    .expect("run misspell");
    let messages: Vec<String> = result
        .diagnostics()
        .into_iter()
        .map(|(_, d)| d.message)
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("brocoli") || m.contains("broccoli")),
        "comment typos should be reported in restricted mode: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("Amercia")),
        "string literal typos should be ignored in restricted mode: {messages:?}"
    );
}
