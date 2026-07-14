mod support;

use guff_error::{durationcheck, err113, errname};

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
