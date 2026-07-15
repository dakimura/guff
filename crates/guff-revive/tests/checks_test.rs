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
        ] {
            assert!(
                messages.iter().any(|m| m.contains(needle)),
                "missing {needle} in {messages:?}"
            );
        }
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
