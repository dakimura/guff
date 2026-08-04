mod support;

use guff_gostaticanalysis::{forcetypeassert, makezero, mirror, nilerr, nilnesserr, nilnil};

#[test]
fn forcetypeassert_flags_unchecked() {
    let dir = support::testdata("forcetypeassert");
    let pkg = support::typecheck_pkg(
        "example.com/forcetypeassert",
        &dir.join("bad.go"),
    );
    let messages = support::run_analyzer(forcetypeassert(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("type assertion must be checked")),
        "{messages:?}"
    );
}

#[test]
fn forcetypeassert_allows_checked() {
    let dir = support::testdata("forcetypeassert");
    let pkg = support::typecheck_pkg(
        "example.com/forcetypeassert/ok",
        &dir.join("ok.go"),
    );
    assert!(support::run_analyzer(forcetypeassert(), &pkg).is_empty());
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

#[test]
fn nilerr_allows_correct_returns() {
    let dir = support::testdata("nilerr");
    let pkg = support::typecheck_pkg("example.com/nilerr/ok", &dir.join("ok.go"));
    let messages = support::run_analyzer(nilerr(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}
