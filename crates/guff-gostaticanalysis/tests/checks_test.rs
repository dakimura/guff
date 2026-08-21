mod support;

use guff_gostaticanalysis::{forcetypeassert, makezero, mirror, nilerr, nilnesserr, nilnil};

/// Every path that reports: a blank assignment, a `:=` and a `=` whose operand
/// is an index expression, a `var` spec, a bare assertion in a condition and one
/// in an argument, and the two ways to earn "right hand must be only type
/// assertion" (buried in a call, or two values on the right).
///
/// The columns are what this fixture is really for, and only the golden case
/// (`compat/golden/cases/forcetypeassert`) compares them — upstream reports
/// `n.Pos()` throughout, which is the same *line* as the `:=` guff used to
/// point at.
#[test]
fn forcetypeassert_flags_unchecked() {
    let dir = support::testdata("forcetypeassert");
    let pkg = support::typecheck_pkg(
        "example.com/forcetypeassert",
        &dir.join("bad.go"),
    );
    let messages = support::run_analyzer(forcetypeassert(), &pkg);
    let mut got: Vec<&String> = messages.iter().collect();
    got.sort();
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.as_str() == "type assertion must be checked")
            .count(),
        6,
        "{got:?}"
    );
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.as_str() == "right hand must be only type assertion")
            .count(),
        2,
        "{got:?}"
    );
    assert_eq!(messages.len(), 8, "{got:?}");
}

/// The silent side: both comma-ok spellings, an assertion to `any` (upstream's
/// `isAny`), and a type switch, whose `TypeAssertExpr` has no `Type` at all.
#[test]
fn forcetypeassert_allows_checked() {
    let dir = support::testdata("forcetypeassert");
    let pkg = support::typecheck_pkg(
        "example.com/forcetypeassert/ok",
        &dir.join("ok.go"),
    );
    let messages = support::run_analyzer(forcetypeassert(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn nilnil_flags_nil_nil_return() {
    let dir = support::testdata("nilnil");
    let pkg = support::typecheck_pkg("example.com/nilnil", &dir.join("bad.go"));
    let messages = support::run_analyzer(nilnil(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("nil") && m.contains("error")),
        "{messages:?}"
    );
}

/// A `return` the rule declines to judge takes its subtree with it.
///
/// Upstream's callback `return false`s on every path out of the ReturnStmt arm
/// — after reporting, and on each rejection before it — and
/// `inspector.Nodes` reads that as "do not descend". So the `nil, nil` inside a
/// func literal that is itself returned is never visited:
///
/// ```go
/// return db.WithTx2(ctx, func(…) (*Comment, error) {
///     return nil, nil          // the outer return has one result
/// })
/// ```
///
/// gitea writes six of those and golangci-lint reports none.
#[test]
fn nilnil_does_not_descend_into_a_rejected_return() {
    let dir = support::testdata("nilnil");
    let pkg = support::typecheck_pkg("example.com/nilnil/ok", &dir.join("ok.go"));
    let messages = support::run_analyzer(nilnil(), &pkg);
    assert!(
        messages.is_empty(),
        "a nil,nil inside a returned func literal is not reached: {messages:?}"
    );
}

#[test]
fn nilnil_allows_valid_returns() {
    let dir = support::testdata("nilnil");
    let pkg = support::typecheck_pkg("example.com/nilnil/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(nilnil(), &pkg).is_empty());
}

#[test]
fn makezero_flags_append_to_nonzero_slice() {
    let dir = support::testdata("makezero");
    let pkg = support::typecheck_pkg("example.com/makezero", &dir.join("bad.go"));
    let messages = support::run_analyzer(makezero(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("non-zero initialized length")),
        "{messages:?}"
    );
}

#[test]
fn makezero_allows_zero_length_make() {
    let dir = support::testdata("makezero");
    let pkg = support::typecheck_pkg("example.com/makezero/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(makezero(), &pkg).is_empty());
}

#[test]
fn mirror_flags_string_byte_conversions() {
    let pkg = support::typecheck_fixture("mirror", "example.com/mirror", "bad.go");
    let messages = support::run_analyzer(mirror(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("avoid allocations with bytes.Compare")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("avoid allocations with utf8.RuneCountInString")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("avoid allocations with bytes.Contains")),
        "{messages:?}"
    );
}

#[test]
fn mirror_allows_native_string_apis() {
    let pkg = support::typecheck_fixture("mirror", "example.com/mirror/ok", "ok.go");
    let messages = support::run_analyzer(mirror(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn mirror_flags_regexp_methods() {
    let pkg = support::typecheck_fixture("mirror", "example.com/mirror/re", "regexp_bad.go");
    let messages = support::run_analyzer(mirror(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("(*regexp.Regexp).MatchString")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("(*regexp.Regexp).Match")),
        "{messages:?}"
    );
}

#[test]
fn nilnesserr_flags_nil_error_after_check() {
    let dir = support::testdata("nilnesserr");
    let pkg = support::typecheck_pkg("example.com/nilnesserr", &dir.join("bad.go"));
    let messages = support::run_analyzer(nilnesserr(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("return a nil value error after check error")),
        "expected return finding, got {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("call function with a nil value error after check error")),
        "expected call finding, got {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("variadic")),
        "expected variadic finding, got {messages:?}"
    );
}

#[test]
fn nilnesserr_allows_correct_error_returns() {
    let dir = support::testdata("nilnesserr");
    let pkg = support::typecheck_pkg("example.com/nilnesserr/ok", &dir.join("ok.go"));
    let messages = support::run_analyzer(nilnesserr(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn nilerr_flags_nil_returns() {
    let dir = support::testdata("nilerr");
    let pkg = support::typecheck_pkg("example.com/nilerr", &dir.join("bad.go"));
    let messages = support::run_analyzer(nilerr(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("error is not nil (line ") && m.contains("returns nil")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("error is nil (line ") && m.contains("returns error")),
        "{messages:?}"
    );
}

/// What counts as "the block uses the error" is `callInstr.Call.Args`, peeled
/// through `MakeInterface` / `ChangeInterface` / an invoke's receiver.
///
/// Boxing is the case that matters: any variadic call — `fmt.Sprintf("…: %v",
/// err)`, `fmt.Errorf("…: %w", err)` — passes a `MakeInterface` wrapping the
/// error, and without peeling it the block reads as though it never mentioned
/// the error at all (25 findings on dapr that golangci-lint does not make).
///
/// The other direction is `err.Error()` on its own: an invoke call keeps its
/// receiver in `Call.Value`, not in `Args`, so upstream does *not* count it,
/// and counting it silenced a finding golangci-lint makes.
#[test]
fn nilerr_use_of_the_error_is_read_off_the_call_arguments() {
    let dir = support::testdata("nilerr");

    let ok = support::typecheck_pkg("example.com/nilerr/ok", &dir.join("ok.go"));
    let ok_messages = support::run_analyzer(nilerr(), &ok);
    assert!(
        ok_messages.is_empty(),
        "an error boxed into `any` for a variadic call is a use: {ok_messages:?}"
    );

    let bad = support::typecheck_pkg("example.com/nilerr", &dir.join("bad.go"));
    let bad_messages = support::run_analyzer(nilerr(), &bad);
    assert_eq!(
        bad_messages
            .iter()
            .filter(|m| m.contains("error is not nil (line "))
            .count(),
        4,
        "the three added blocks — err.Error() only, err copied to a local, and \
         a nil in a real error position — are findings alongside the original: \
         {bad_messages:?}"
    );
}

/// A nil is only a swallowed error if it sits in an **error result position**.
///
/// Upstream counts `ret.Results` that implement `error`; a function with none —
/// `func value(raw []byte) (*float64, bool)` returning `nil, false` — has
/// `errorReturnValues == 0` and is not a finding. guff typed the untyped nil as
/// an error and reported six of these in one jaeger file.
#[test]
fn nilerr_nil_must_be_in_an_error_result_position() {
    let dir = support::testdata("nilerr");

    let ok = support::typecheck_pkg("example.com/nilerr/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(nilerr(), &ok).is_empty(),
        "a function with no error result is not a finding"
    );

    let bad = support::typecheck_pkg("example.com/nilerr", &dir.join("bad.go"));
    let messages = support::run_analyzer(nilerr(), &bad);
    assert!(
        messages
            .iter()
            .filter(|m| m.contains("error is not nil"))
            .count()
            >= 4,
        "a nil in a real error position still is: {messages:?}"
    );
}

#[test]
fn nilerr_allows_correct_returns() {
    let dir = support::testdata("nilerr");
    let pkg = support::typecheck_pkg("example.com/nilerr/ok", &dir.join("ok.go"));
    let messages = support::run_analyzer(nilerr(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}
