mod support;

use guff_analysis::validate;
use guff_misspell::misspell;

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
