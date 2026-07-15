mod support;

use guff_analysis::validate;
use guff_revive::revive;

#[test]
fn revive_flags_default_rule_violations() {
    let pkg = support::typecheck_fixture("revive", "example.com/revive", "bad.go");
    let messages = support::run_analyzer(revive(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("dot-imports:")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("blank-imports:")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("increment-decrement:")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("error-naming:")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("error-strings:")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("redefines-builtin-id:")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("receiver-naming:")),
        "{messages:?}"
    );
}

#[test]
fn revive_allows_clean_code() {
    let pkg = support::typecheck_fixture("revive", "example.com/revive/ok", "ok.go");
    assert!(support::run_analyzer(revive(), &pkg).is_empty());
}

#[test]
fn revive_analyzer_graph_is_valid() {
    validate(&[revive()]).expect("valid analyzer graph");
}
