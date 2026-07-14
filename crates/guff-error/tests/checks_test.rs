mod support;

use guff_error::{durationcheck, err113, errchkjson, errname, errorlint, wrapcheck};

#[test]
fn errname_flags_bad_type_and_var_names() {
    let dir = support::testdata("errname");
    let pkg = support::typecheck_pkg("example.com/errname", &dir.join("bad.go"));
    let messages = support::run_analyzer(errname(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("error type name")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("sentinel error name")),
        "{messages:?}"
    );
}

#[test]
fn errname_allows_valid_names() {
    let dir = support::testdata("errname");
    let pkg = support::typecheck_pkg("example.com/errname/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(errname(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(errname(), &pkg)
    );
}

#[test]
fn err113_flags_direct_error_comparison() {
    let dir = support::testdata("err113");
    let pkg = support::typecheck_pkg("example.com/err113", &dir.join("bad.go"));
    let messages = support::run_analyzer(err113(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("do not compare errors directly")),
        "{messages:?}"
    );
}

#[test]
fn err113_allows_errors_is_style() {
    let dir = support::testdata("err113");
    let pkg = support::typecheck_pkg("example.com/err113/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(err113(), &pkg).is_empty());
}

#[test]
fn durationcheck_flags_duration_times_duration() {
    let dir = support::testdata("durationcheck");
    let pkg = support::typecheck_pkg("example.com/durationcheck", &dir.join("bad.go"));
    let messages = support::run_analyzer(durationcheck(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Multiplication of durations")),
        "{messages:?}"
    );
}

#[test]
fn durationcheck_allows_duration_times_int_cast() {
    let dir = support::testdata("durationcheck");
    let pkg = support::typecheck_pkg("example.com/durationcheck/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(durationcheck(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(durationcheck(), &pkg)
    );
}

#[test]
fn errorlint_flags_error_comparison() {
    let dir = support::testdata("errorlint");
    let pkg = support::typecheck_pkg("example.com/errorlint", &dir.join("bad.go"));
    let messages = support::run_analyzer(errorlint(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("comparing with") && m.contains("errors.Is")),
        "{messages:?}"
    );
}

#[test]
fn errorlint_allows_nil_check() {
    let dir = support::testdata("errorlint");
    let pkg = support::typecheck_pkg("example.com/errorlint/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(errorlint(), &pkg).is_empty());
}

#[test]
fn wrapcheck_flags_unwrapped_external_error() {
    let dir = support::testdata("wrapcheck");
    let pkg = support::typecheck_pkg("example.com/wrapcheck", &dir.join("bad.go"));
    let messages = support::run_analyzer(wrapcheck(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("external package") && m.contains("unwrapped")),
        "{messages:?}"
    );
}

#[test]
fn wrapcheck_allows_wrapped_error() {
    let dir = support::testdata("wrapcheck");
    let pkg = support::typecheck_pkg("example.com/wrapcheck/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(wrapcheck(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(wrapcheck(), &pkg)
    );
}

#[test]
fn errchkjson_flags_blank_and_unsupported() {
    let dir = support::testdata("errchkjson");
    let pkg = support::typecheck_pkg("example.com/errchkjson", &dir.join("bad.go"));
    let messages = support::run_analyzer(errchkjson(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("encoding/json.Marshal") && m.contains("is not checked")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("unsupported type") && m.contains("chan")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("unsafe type") && m.contains("float64")),
        "{messages:?}"
    );
}

#[test]
fn errchkjson_allows_checked_safe_and_unsafe() {
    let dir = support::testdata("errchkjson");
    let pkg = support::typecheck_pkg("example.com/errchkjson/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(errchkjson(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(errchkjson(), &pkg)
    );
}
