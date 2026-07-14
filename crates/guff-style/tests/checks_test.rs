mod support;

use guff_style::{
    asciicheck, copyloopvar, dogsled, funlen, goconst, gocyclo, goprintffuncname, lll, perfsprint,
    usestdlibvars, usetesting,
};

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

#[test]
fn dogsled_flags_too_many_blanks() {
    let pkg = support::typecheck_fixture("dogsled", "example.com/dogsled", "bad.go");
    let messages = support::run_analyzer(dogsled(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("declaration has 3 blank identifiers")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("declaration has 4 blank identifiers")),
        "{messages:?}"
    );
}

#[test]
fn dogsled_allows_two_or_fewer_blanks() {
    let pkg = support::typecheck_fixture("dogsled", "example.com/dogsled/ok", "ok.go");
    assert!(support::run_analyzer(dogsled(), &pkg).is_empty());
}

#[test]
fn asciicheck_flags_non_ascii_idents() {
    let pkg = support::typecheck_fixture("asciicheck", "example.com/asciicheck", "bad.go");
    let messages = support::run_analyzer(asciicheck(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("TéstFunc") && m.contains("non-ASCII")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("téstConst") && m.contains("non-ASCII")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("téstParam") && m.contains("non-ASCII")),
        "{messages:?}"
    );
}

#[test]
fn asciicheck_allows_ascii_idents() {
    let pkg = support::typecheck_fixture("asciicheck", "example.com/asciicheck/ok", "ok.go");
    assert!(support::run_analyzer(asciicheck(), &pkg).is_empty());
}

#[test]
fn goprintffuncname_flags_missing_f_suffix() {
    let pkg = support::typecheck_fixture(
        "goprintffuncname",
        "example.com/goprintffuncname",
        "bad.go",
    );
    let messages = support::run_analyzer(goprintffuncname(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("prinfLikeFunc") && m.contains("prinfLikeFuncf")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("prinfLikeFuncAny") && m.contains("should be named")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("prinfLikeFuncWithExtraArgs")),
        "{messages:?}"
    );
}

#[test]
fn goprintffuncname_allows_correct_names() {
    let pkg = support::typecheck_fixture(
        "goprintffuncname",
        "example.com/goprintffuncname/ok",
        "ok.go",
    );
    assert!(support::run_analyzer(goprintffuncname(), &pkg).is_empty());
}

#[test]
fn funlen_flags_too_many_statements() {
    let pkg = support::typecheck_fixture("funlen", "example.com/funlen", "bad.go");
    let messages = support::run_analyzer(funlen(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("TooManyStatements") && m.contains("too many statements")),
        "{messages:?}"
    );
}

#[test]
fn funlen_allows_short_functions() {
    let pkg = support::typecheck_fixture("funlen", "example.com/funlen/ok", "ok.go");
    assert!(support::run_analyzer(funlen(), &pkg).is_empty());
}

#[test]
fn gocyclo_flags_high_complexity() {
    let pkg = support::typecheck_fixture("gocyclo", "example.com/gocyclo", "bad.go");
    let messages = support::run_analyzer(gocyclo(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("HighComplexity") && m.contains("cyclomatic complexity")),
        "{messages:?}"
    );
}

#[test]
fn gocyclo_allows_low_complexity() {
    let pkg = support::typecheck_fixture("gocyclo", "example.com/gocyclo/ok", "ok.go");
    assert!(support::run_analyzer(gocyclo(), &pkg).is_empty());
}

#[test]
fn lll_flags_long_lines() {
    let pkg = support::typecheck_fixture("lll", "example.com/lll", "bad.go");
    let messages = support::run_analyzer(lll(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("characters long") && m.contains("120")),
        "{messages:?}"
    );
}

#[test]
fn lll_allows_short_lines() {
    let pkg = support::typecheck_fixture("lll", "example.com/lll/ok", "ok.go");
    assert!(support::run_analyzer(lll(), &pkg).is_empty());
}
