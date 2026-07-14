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
fn dupword_flags_duplicates_in_comments_and_strings() {
    let pkg = support::typecheck_fixture("dupword", "example.com/dupword", "bad.go");
    let messages = support::run_analyzer(dupword(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("Duplicate words") && m.contains("is")),
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
