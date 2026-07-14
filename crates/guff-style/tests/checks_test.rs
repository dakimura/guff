mod support;

use guff_style::{copyloopvar, goconst, perfsprint, usestdlibvars, usetesting};

#[test]
fn copyloopvar_flags_redundant_copies() {
    let pkg = support::typecheck_fixture(
        "copyloopvar",
        "example.com/copyloopvar",
        "bad.go",
    );
    let messages = support::run_analyzer(copyloopvar(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("for") && m.contains("\"i\"")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("for") && m.contains("\"v\"")),
        "{messages:?}"
    );
}

#[test]
fn copyloopvar_allows_alias_copies() {
    let pkg = support::typecheck_fixture(
        "copyloopvar",
        "example.com/copyloopvar/ok",
        "ok.go",
    );
    assert!(support::run_analyzer(copyloopvar(), &pkg).is_empty());
}

#[test]
fn usetesting_flags_os_mkdirtemp_and_createtemp() {
    let pkg = support::typecheck_fixture("usetesting", "example.com/usetesting", "bad.go");
    let messages = support::run_analyzer(usetesting(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("os.MkdirTemp") && m.contains("t.TempDir")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("os.CreateTemp") && m.contains("t.TempDir")),
        "{messages:?}"
    );
}

#[test]
fn usetesting_allows_testing_helpers() {
    let pkg = support::typecheck_fixture("usetesting", "example.com/usetesting/ok", "ok.go");
    assert!(support::run_analyzer(usetesting(), &pkg).is_empty());
}

#[test]
fn usestdlibvars_flags_http_literals() {
    let pkg = support::typecheck_fixture(
        "usestdlibvars",
        "example.com/usestdlibvars",
        "bad.go",
    );
    let messages = support::run_analyzer(usestdlibvars(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("http.MethodGet")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("http.StatusNotFound")),
        "{messages:?}"
    );
}

#[test]
fn usestdlibvars_allows_stdlib_constants() {
    let pkg = support::typecheck_fixture(
        "usestdlibvars",
        "example.com/usestdlibvars/ok",
        "ok.go",
    );
    assert!(support::run_analyzer(usestdlibvars(), &pkg).is_empty());
}

#[test]
fn perfsprint_flags_fmt_shortcuts() {
    let pkg = support::typecheck_fixture("perfsprint", "example.com/perfsprint", "bad.go");
    let messages = support::run_analyzer(perfsprint(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("string-format") && m.contains("fmt.Sprintf")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("error-format") && m.contains("errors.New")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("bool-format") && m.contains("FormatBool")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("integer-format") && m.contains("Itoa")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("hex-format") && m.contains("EncodeToString")),
        "{messages:?}"
    );
}

#[test]
fn perfsprint_allows_complex_fmt() {
    let pkg = support::typecheck_fixture("perfsprint", "example.com/perfsprint/ok", "ok.go");
    assert!(support::run_analyzer(perfsprint(), &pkg).is_empty());
}

#[test]
fn goconst_flags_repeated_strings() {
    let pkg = support::typecheck_fixture("goconst", "example.com/goconst", "bad.go");
    let messages = support::run_analyzer(goconst(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("needconst") && m.contains("3 occurrences")),
        "{messages:?}"
    );
}

#[test]
fn goconst_allows_below_threshold() {
    let pkg = support::typecheck_fixture("goconst", "example.com/goconst/ok", "ok.go");
    assert!(support::run_analyzer(goconst(), &pkg).is_empty());
}
