mod support;

use guff_style::{
    asciicheck, copyloopvar, cyclop, dogsled, funlen, gocognit, goconst, gocyclo,
    goprintffuncname, lll, mnd, nakedret, nestif, nlreturn, nosprintfhostport, perfsprint,
    prealloc, predeclared, tagalign, usestdlibvars, usetesting, whitespace, wsl,
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

#[test]
fn gocognit_flags_high_cognitive_complexity() {
    let pkg = support::typecheck_fixture("gocognit", "example.com/gocognit", "bad.go");
    let messages = support::run_analyzer(gocognit(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("HighCognitive") && m.contains("cognitive complexity")),
        "{messages:?}"
    );
}

#[test]
fn gocognit_allows_low_cognitive_complexity() {
    let pkg = support::typecheck_fixture("gocognit", "example.com/gocognit/ok", "ok.go");
    assert!(support::run_analyzer(gocognit(), &pkg).is_empty());
}

#[test]
fn nestif_flags_deep_nesting() {
    let pkg = support::typecheck_fixture("nestif", "example.com/nestif", "bad.go");
    let messages = support::run_analyzer(nestif(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("complex nested blocks") && m.contains("if a")),
        "{messages:?}"
    );
}

#[test]
fn nestif_allows_shallow_nesting() {
    let pkg = support::typecheck_fixture("nestif", "example.com/nestif/ok", "ok.go");
    assert!(support::run_analyzer(nestif(), &pkg).is_empty());
}

#[test]
fn cyclop_flags_high_complexity() {
    let pkg = support::typecheck_fixture("cyclop", "example.com/cyclop", "bad.go");
    let messages = support::run_analyzer(cyclop(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("HighComplexity") && m.contains("cyclomatic complexity")),
        "{messages:?}"
    );
}

#[test]
fn cyclop_allows_low_complexity() {
    let pkg = support::typecheck_fixture("cyclop", "example.com/cyclop/ok", "ok.go");
    assert!(support::run_analyzer(cyclop(), &pkg).is_empty());
}

#[test]
fn nakedret_flags_long_naked_returns() {
    let pkg = support::typecheck_fixture("nakedret", "example.com/nakedret", "bad.go");
    let messages = support::run_analyzer(nakedret(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("naked return") && m.contains("LongNamed")),
        "{messages:?}"
    );
}

#[test]
fn nakedret_allows_short_or_explicit() {
    let pkg = support::typecheck_fixture("nakedret", "example.com/nakedret/ok", "ok.go");
    assert!(support::run_analyzer(nakedret(), &pkg).is_empty());
}

#[test]
fn nosprintfhostport_flags_host_port_sprintf() {
    let pkg = support::typecheck_fixture(
        "nosprintfhostport",
        "example.com/nosprintfhostport",
        "bad.go",
    );
    let messages = support::run_analyzer(nosprintfhostport(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("net.JoinHostPort") && m.contains("fmt.Sprintf")),
        "{messages:?}"
    );
    assert!(
        messages.len() >= 2,
        "expected both host:port and auth URL hits, got {messages:?}"
    );
}

#[test]
fn nosprintfhostport_allows_safe_sprintf() {
    let pkg = support::typecheck_fixture(
        "nosprintfhostport",
        "example.com/nosprintfhostport/ok",
        "ok.go",
    );
    assert!(support::run_analyzer(nosprintfhostport(), &pkg).is_empty());
}

#[test]
fn predeclared_flags_shadowed_identifiers() {
    let pkg = support::typecheck_fixture("predeclared", "example.com/predeclared", "bad.go");
    let messages = support::run_analyzer(predeclared(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("function len") && m.contains("predeclared")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("variable error") && m.contains("predeclared")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("variable true") && m.contains("predeclared")),
        "{messages:?}"
    );
}

#[test]
fn predeclared_allows_non_shadowing_names() {
    let pkg = support::typecheck_fixture("predeclared", "example.com/predeclared/ok", "ok.go");
    assert!(support::run_analyzer(predeclared(), &pkg).is_empty());
}

#[test]
fn whitespace_flags_leading_and_trailing() {
    let pkg = support::typecheck_fixture("whitespace", "example.com/whitespace", "bad.go");
    let messages = support::run_analyzer(whitespace(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("unnecessary leading newline")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("unnecessary trailing newline")),
        "{messages:?}"
    );
}

#[test]
fn whitespace_allows_tight_blocks() {
    let pkg = support::typecheck_fixture("whitespace", "example.com/whitespace/ok", "ok.go");
    assert!(support::run_analyzer(whitespace(), &pkg).is_empty());
}

#[test]
fn nlreturn_flags_missing_blank_before_return() {
    let pkg = support::typecheck_fixture("nlreturn", "example.com/nlreturn", "bad.go");
    let messages = support::run_analyzer(nlreturn(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("return with no blank line before")),
        "{messages:?}"
    );
}

#[test]
fn nlreturn_allows_alone_or_blanked_returns() {
    let pkg = support::typecheck_fixture("nlreturn", "example.com/nlreturn/ok", "ok.go");
    assert!(support::run_analyzer(nlreturn(), &pkg).is_empty());
}

#[test]
fn mnd_flags_magic_numbers() {
    let pkg = support::typecheck_fixture("mnd", "example.com/mnd", "bad.go");
    let messages = support::run_analyzer(mnd(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Magic number") && m.contains("<condition>")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Magic number") && m.contains("<argument>")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Magic number") && m.contains("<return>")),
        "{messages:?}"
    );
}

#[test]
fn mnd_allows_ignored_literals() {
    let pkg = support::typecheck_fixture("mnd", "example.com/mnd/ok", "ok.go");
    assert!(support::run_analyzer(mnd(), &pkg).is_empty());
}

#[test]
fn prealloc_flags_range_append() {
    let pkg = support::typecheck_fixture("prealloc", "example.com/prealloc", "bad.go");
    let messages = support::run_analyzer(prealloc(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Consider preallocating dest")),
        "{messages:?}"
    );
}

#[test]
fn prealloc_allows_make_capacity() {
    let pkg = support::typecheck_fixture("prealloc", "example.com/prealloc/ok", "ok.go");
    assert!(support::run_analyzer(prealloc(), &pkg).is_empty());
}

#[test]
fn tagalign_flags_misaligned_tags() {
    let pkg = support::typecheck_fixture("tagalign", "example.com/tagalign", "bad.go");
    let messages = support::run_analyzer(tagalign(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("tag is not aligned")),
        "{messages:?}"
    );
}

#[test]
fn tagalign_allows_aligned_sorted_tags() {
    let pkg = support::typecheck_fixture("tagalign", "example.com/tagalign/ok", "ok.go");
    assert!(support::run_analyzer(tagalign(), &pkg).is_empty());
}

#[test]
fn wsl_flags_cuddle_violations() {
    let pkg = support::typecheck_fixture("wsl", "example.com/wsl", "bad.go");
    let messages = support::run_analyzer(wsl(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("if statements should only be cuddled with assignments")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("used in the if statement itself")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("only one cuddle assignment allowed before if")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("declarations should never be cuddled")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("assignments should only be cuddled with other assignments")),
        "{messages:?}"
    );
}

#[test]
fn wsl_allows_proper_spacing() {
    let pkg = support::typecheck_fixture("wsl", "example.com/wsl/ok", "ok.go");
    let messages = support::run_analyzer(wsl(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}
