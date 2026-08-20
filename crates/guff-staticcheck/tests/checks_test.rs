mod support;

use guff_staticcheck::{sa4000, sa4001, sa4003, sa4004, sa4005, sa4006, sa4008, sa4009, sa4010, sa4011, sa4012, sa4013, sa4014, sa4015, sa4016, sa4017, sa4018, sa4019, sa4020, sa4021, sa4022, sa4023, sa4024, sa4025, sa4026, sa4027, sa4028, sa4029, sa4030, sa4031, sa4032, sa1000, sa1001, sa1002, sa1003, sa1004, sa1005, sa1006, sa1007, sa1008, sa1010, sa1011, sa1012, sa1013, sa1014, sa1015, sa1016, sa1017, sa1018, sa1019, sa1020, sa1021, sa1023, sa1024, sa1025, sa1026, sa1027, sa1028, sa1029, sa1030, sa1031, sa1032, sa2000, sa2001, sa2002, sa2003, sa3000, sa3001, sa5000, sa5001, sa5002, sa5003, sa5004, sa5005, sa5007, sa5008, sa5009, sa5010, sa5011, sa5012, sa6000, sa6001, sa6002, sa6003, sa6005, sa6006, sa9001, sa9002, sa9003, sa9004, sa9005, sa9006, sa9007, sa9008, sa9009, s1000, s1001, s1003, s1004, s1005, s1006, s1007, s1008, s1009, s1010, s1011, s1012, s1016, s1017, s1018, s1019, s1020, s1021, s1023, s1024, s1025, s1028, s1029, s1030, s1031, s1032, s1033, s1034, s1035, s1036, s1037, s1038, s1039, s1040, st1000, st1001, st1003, st1005, st1006, st1008, st1011, st1012, st1013, st1015, st1016, st1017, st1018, st1019, st1020, st1021, st1022, st1023, qf1001, qf1002, qf1003, qf1004, qf1005, qf1006, qf1007, qf1008, qf1009, qf1010, qf1011, qf1012};
use guff_types::sizes_for;

#[test]
fn sa1017_flags_unbuffered_notify_channel() {
    let dir = support::testdata("sa1017");
    let os_stub = dir.join("stub/os/os.go");
    let signal_stub = dir.join("stub/os/signal/signal.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1017",
        &dir.join("bad.go"),
        &[("os", &os_stub), ("os/signal", &signal_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1017::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("should be buffered"));
}

#[test]
fn sa1017_allows_buffered_notify_channel() {
    let dir = support::testdata("sa1017");
    let os_stub = dir.join("stub/os/os.go");
    let signal_stub = dir.join("stub/os/signal/signal.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1017/ok",
        &dir.join("ok.go"),
        &[("os", &os_stub), ("os/signal", &signal_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1017::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1012_flags_nil_context_first_arg() {
    let dir = support::testdata("sa1012");
    let ctx_stub = dir.join("stub/context/context.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1012",
        &dir.join("bad.go"),
        &[("context", &ctx_stub)],
    );
    assert!(pkg.types_info.is_some(), "missing types info");
    let messages = support::run_analyzer(sa1012::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("nil Context"));
}

#[test]
fn sa1012_allows_todo_or_non_context_nil() {
    let dir = support::testdata("sa1012");
    let ctx_stub = dir.join("stub/context/context.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1012/ok",
        &dir.join("ok.go"),
        &[("context", &ctx_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1012::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1013_flags_swapped_seek_arguments() {
    let dir = support::testdata("sa1013");
    let io_stub = dir.join("stub/io/io.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1013",
        &dir.join("bad.go"),
        &[("io", &io_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1013::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("io.Seek*"));
}

#[test]
fn sa1013_allows_correct_seek_arguments() {
    let dir = support::testdata("sa1013");
    let io_stub = dir.join("stub/io/io.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1013/ok",
        &dir.join("ok.go"),
        &[("io", &io_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1013::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1026_flags_unmarshalable_types() {
    let dir = support::testdata("sa1026");
    let json_stub = dir.join("stub/encoding/json/json.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1026",
        &dir.join("bad.go"),
        &[("encoding/json", &json_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1026::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("chan int")));
    assert!(messages.iter().any(|m| m.contains("func()")));
}

#[test]
fn sa1026_allows_marshalable_types() {
    let dir = support::testdata("sa1026");
    let json_stub = dir.join("stub/encoding/json/json.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1026/ok",
        &dir.join("ok.go"),
        &[("encoding/json", &json_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1026::analyzer(), &pkg).is_empty());
}

#[test]
fn s1017_flags_manual_trimming() {
    let dir = support::testdata("s1017");
    let strings_stub = dir.join("stub/strings/strings.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1017",
        &dir.join("bad.go"),
        &[("strings", &strings_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1017::analyzer(), &pkg);
    assert!(messages.len() >= 2, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("TrimPrefix")));
}

#[test]
fn s1017_allows_other_patterns() {
    let dir = support::testdata("s1017");
    let strings_stub = dir.join("stub/strings/strings.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1017/ok",
        &dir.join("ok.go"),
        &[("strings", &strings_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1017::analyzer(), &pkg).is_empty());
}

#[test]
fn s1021_flags_mergeable_decl_assign() {
    let dir = support::testdata("s1021");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1021");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1021::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("merge variable declaration"));
}

#[test]
fn s1021_allows_merged_or_reassigned() {
    let dir = support::testdata("s1021");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1021/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1021::analyzer(), &pkg).is_empty());
}

#[test]
fn s1032_flags_sort_slice_wrappers() {
    let dir = support::testdata("s1032");
    let sort_stub = dir.join("stub/sort/sort.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1032",
        &dir.join("bad.go"),
        &[("sort", &sort_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1032::analyzer(), &pkg);
    assert!(messages.len() >= 2, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("sort.Ints")));
    assert!(messages.iter().any(|m| m.contains("sort.Strings")));
}

#[test]
fn s1032_allows_direct_helpers_or_ambiguous() {
    let dir = support::testdata("s1032");
    let sort_stub = dir.join("stub/sort/sort.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1032/ok",
        &dir.join("ok.go"),
        &[("sort", &sort_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1032::analyzer(), &pkg).is_empty());
}

#[test]
fn s1029_flags_rune_slice_range() {
    let dir = support::testdata("s1029");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1029");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1029::analyzer(), &pkg);
    assert!(messages.len() >= 2, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("range over string")));
}

#[test]
fn s1029_allows_direct_string_range() {
    let dir = support::testdata("s1029");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1029/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1029::analyzer(), &pkg).is_empty());
}

#[test]
fn s1003_flags_index_comparisons() {
    let dir = support::testdata("s1003");
    let strings_stub = dir.join("stub/strings/strings.go");
    let bytes_stub = dir.join("stub/bytes/bytes.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1003",
        &dir.join("bad.go"),
        &[("strings", &strings_stub), ("bytes", &bytes_stub)],
    );
    support::assert_well_typed(&pkg);

    let messages = support::run_analyzer(s1003::analyzer(), &pkg);
    assert!(messages.len() >= 4, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("strings.Contains")));
    assert!(messages.iter().any(|m| m.contains("!strings.Contains")));
}

#[test]
fn s1003_allows_non_index_patterns() {
    let dir = support::testdata("s1003");
    let strings_stub = dir.join("stub/strings/strings.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1003/ok",
        &dir.join("ok.go"),
        &[("strings", &strings_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1003::analyzer(), &pkg).is_empty());
}

#[test]
fn s1006_flags_for_true_loops() {
    let dir = support::testdata("s1006");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1006");
    support::assert_well_typed(&pkg);

    let messages = support::run_analyzer(s1006::analyzer(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("for {}")));
}

#[test]
fn s1006_allows_other_loops() {
    let dir = support::testdata("s1006");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1006/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1006::analyzer(), &pkg).is_empty());
}

#[test]
fn s1009_flags_redundant_nil_checks() {
    let dir = support::testdata("s1009");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1009");
    support::assert_well_typed(&pkg);

    let messages = support::run_analyzer(s1009::analyzer(), &pkg);
    assert!(messages.len() >= 4, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("nil slices")));
    assert!(messages.iter().any(|m| m.contains("nil maps")));
    assert!(messages.iter().any(|m| m.contains("nil channels")));
}

#[test]
fn s1009_allows_needed_nil_checks() {
    let dir = support::testdata("s1009");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1009/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1009::analyzer(), &pkg).is_empty());
}

#[test]
fn s1023_flags_redundant_break_and_return() {
    let dir = support::testdata("s1023");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1023");
    support::assert_well_typed(&pkg);

    let messages = support::run_analyzer(s1023::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("redundant break")));
    assert!(messages.iter().any(|m| m.contains("redundant return")));
}

#[test]
fn s1023_allows_needed_control_flow() {
    let dir = support::testdata("s1023");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1023/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1023::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1004_flags_small_sleep_literals() {
    let dir = support::testdata("sa1004");
    let time_stub = dir.join("stub/time/sleep.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1004",
        &dir.join("bad.go"),
        &[("time", &time_stub)],
    );
    support::assert_well_typed(&pkg);

    let messages = support::run_analyzer(sa1004::analyzer(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("sleeping for 1")));
    assert!(messages.iter().any(|m| m.contains("sleeping for 42")));
}

#[test]
fn sa1004_allows_explicit_durations() {
    let dir = support::testdata("sa1004");
    let time_stub = dir.join("stub/time/sleep.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1004/ok",
        &dir.join("ok.go"),
        &[("time", &time_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1004::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1005_flags_shell_like_exec_command() {
    let dir = support::testdata("sa1005");
    let exec_stub = dir.join("stub/os/exec/exec.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1005",
        &dir.join("bad.go"),
        &[("os/exec", &exec_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1005::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("shell command"));
}

#[test]
fn sa1005_allows_program_paths() {
    let dir = support::testdata("sa1005");
    let exec_stub = dir.join("stub/os/exec/exec.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1005/ok",
        &dir.join("ok.go"),
        &[("os/exec", &exec_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1005::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1006_flags_dynamic_printf_format() {
    let dir = support::testdata("sa1006");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1006",
        &dir.join("bad.go"),
        &[("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1006::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("print-style function"));
}

#[test]
fn sa1006_allows_static_or_multi_arg_printf() {
    let dir = support::testdata("sa1006");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1006/ok",
        &dir.join("ok.go"),
        &[("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1006::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1008_flags_non_canonical_header_keys() {
    let dir = support::testdata("sa1008");
    let http_stub = dir.join("stub/net/http/http.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1008",
        &dir.join("bad.go"),
        &[("net/http", &http_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1008::analyzer(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("not canonical")));
}

#[test]
fn sa1008_allows_canonical_header_keys() {
    let dir = support::testdata("sa1008");
    let http_stub = dir.join("stub/net/http/http.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1008/ok",
        &dir.join("ok.go"),
        &[("net/http", &http_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1008::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1000_flags_invalid_regex_patterns() {
    let dir = support::testdata("sa1000");
    let regexp_stub = dir.join("stub/regexp/regexp.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1000",
        &dir.join("bad.go"),
        &[("regexp", &regexp_stub)],
    );
    support::assert_well_typed(&pkg);

    // Pinned in full. SA1000 prints regexp.Compile's error verbatim, so both
    // the code and the Expr it quotes are user-visible — and a substring check
    // for "error parsing regexp" passed for as long as this check was an
    // approximation that got the rest of the line wrong.
    let messages = support::run_analyzer(sa1000::analyzer(), &pkg);
    assert_eq!(
        messages,
        vec![
            "error parsing regexp: missing closing ): `foo(`",
            "error parsing regexp: missing closing ]: `[`",
            "error parsing regexp: unexpected ): `a)`",
            "error parsing regexp: missing argument to repetition operator: `*`",
            "error parsing regexp: invalid nested repetition operator: `**`",
            "error parsing regexp: invalid repeat count: `{2,1}`",
            "error parsing regexp: invalid repeat count: `{1001}`",
            "error parsing regexp: invalid repeat count: `{100}`",
            "error parsing regexp: invalid escape sequence: `\\q`",
            "error parsing regexp: invalid escape sequence: `\\C`",
            "error parsing regexp: trailing backslash at end of expression: ``",
            "error parsing regexp: invalid character class range: `z-a`",
            "error parsing regexp: invalid character class range: `[:foo:]`",
            "error parsing regexp: invalid character class range: `\\p{Foo}`",
            "error parsing regexp: invalid named capture: `(?P<>`",
            "error parsing regexp: invalid named capture: `(?<a.b>`",
            "error parsing regexp: invalid or unsupported Perl syntax: `(?=`",
            // A Go string is bytes, so these reach the scanner as the
            // ill-formed bytes they name. `(\xff` reports the bad byte rather
            // than the unclosed group: `regexp/syntax` checks UTF-8 while
            // lexing, before it ever sees the `(`.
            //
            // U+FFFD, not the byte: the Expr is the ill-formed tail verbatim,
            // and a Rust `String` cannot hold it. One replacement character per
            // *byte*, which is also what Go's own `encoding/json` does — so the
            // golden tier (JSON on both sides) agrees, and only golangci's text
            // output, which passes the raw bytes through, differs. See
            // docs/COMPAT-HARDENING.md §7.
            "error parsing regexp: invalid UTF-8: `\u{fffd}`",
            "error parsing regexp: invalid UTF-8: `\u{fffd}b`",
            "error parsing regexp: invalid UTF-8: `\u{fffd}`",
            "error parsing regexp: invalid UTF-8: `\u{fffd}\u{fffd}\u{fffd}`",
            "error parsing regexp: invalid UTF-8: `\u{fffd}`",
            // Truncated lead bytes: `\xc3` wants one continuation byte and
            // `\xe2\x82` wants two, so the second one is two bytes long.
            "error parsing regexp: invalid UTF-8: `\u{fffd}`",
            "error parsing regexp: invalid UTF-8: `\u{fffd}\u{fffd}`",
            // The operator is inside the quoted tail, not a second error.
            "error parsing regexp: invalid UTF-8: `\u{fffd}*`",
            "error parsing regexp: invalid UTF-8: `\u{fffd}{2}`",
            "error parsing regexp: missing argument to repetition operator: `+`",
            "error parsing regexp: invalid named capture: `(?P<>`",
            "error parsing regexp: invalid escape sequence: `\\d`",
        ],
        "{messages:?}"
    );
}

#[test]
fn sa1000_allows_valid_patterns() {
    let dir = support::testdata("sa1000");
    let regexp_stub = dir.join("stub/regexp/regexp.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1000/ok",
        &dir.join("ok.go"),
        &[("regexp", &regexp_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1000::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1002_flags_invalid_parse_layouts() {
    let dir = support::testdata("sa1002");
    let time_stub = dir.join("stub/time/parse.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1002",
        &dir.join("bad.go"),
        &[("time", &time_stub)],
    );
    support::assert_well_typed(&pkg);

    // Upstream reports `err.Error()` verbatim, so pin the whole string: a
    // `contains("parsing time")` assertion would pass on any wording.
    // Expectations are Go's own output for `time.Parse(s, s)`.
    let messages = support::run_analyzer(sa1002::analyzer(), &pkg);
    assert_eq!(
        messages,
        vec![
            r#"parsing time "12345" as "12345": cannot parse "" as "4""#.to_string(),
            r#"parsing time "1234" as "1234": cannot parse "" as "3""#.to_string(),
            r#"parsing time "123456": hour out of range"#.to_string(),
            // The layout is bytes and ParseError quotes it, so an ill-formed
            // one has to print as `\xff` — the single byte, not the three of
            // a U+FFFD.
            r#"parsing time "12345\xff" as "12345\xff": cannot parse "\xff" as "4""#.to_string(),
            r#"parsing time "\xff1234" as "\xff1234": cannot parse "" as "3""#.to_string(),
        ],
    );
}

#[test]
fn sa1002_allows_valid_layouts() {
    let dir = support::testdata("sa1002");
    let time_stub = dir.join("stub/time/parse.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1002/ok",
        &dir.join("ok.go"),
        &[("time", &time_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1002::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1024_flags_duplicate_cutsets() {
    let dir = support::testdata("sa1024");
    let strings_stub = dir.join("stub/strings/strings.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1024",
        &dir.join("bad.go"),
        &[("strings", &strings_stub)],
    );
    support::assert_well_typed(&pkg);

    let messages = support::run_analyzer(sa1024::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("cutset contains duplicate characters"))
    );
}

#[test]
fn sa1024_allows_unique_cutsets() {
    let dir = support::testdata("sa1024");
    let strings_stub = dir.join("stub/strings/strings.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1024/ok",
        &dir.join("ok.go"),
        &[("strings", &strings_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1024::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1018_flags_replace_with_zero_count() {
    let dir = support::testdata("sa1018");
    let strings_stub = dir.join("stub/strings/strings.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1018",
        &dir.join("bad.go"),
        &[("strings", &strings_stub)],
    );
    support::assert_well_typed(&pkg);

    let messages = support::run_analyzer(sa1018::analyzer(), &pkg);
    assert!(messages.len() >= 2, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("n == 0 will return no results"))
    );
}

#[test]
fn sa1018_allows_nonzero_replace_count() {
    let dir = support::testdata("sa1018");
    let strings_stub = dir.join("stub/strings/strings.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1018/ok",
        &dir.join("ok.go"),
        &[("strings", &strings_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1018::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1007_flags_invalid_urls() {
    let dir = support::testdata("sa1007");
    let url_stub = dir.join("stub/net/url/url.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1007",
        &dir.join("bad.go"),
        &[("net/url", &url_stub)],
    );
    support::assert_well_typed(&pkg);
    // Pin the whole message: `contains("is not a valid URL")` only checks
    // staticcheck's own wrapper, never the `net/url` error inside it — which
    // is the part that used to come from a Rust crate and match nothing.
    // Expectations are golangci-lint 2.12.2's output (compat/golden).
    let messages = support::run_analyzer(sa1007::analyzer(), &pkg);
    assert_eq!(
        messages,
        vec![
            r#"":" is not a valid URL: parse ":": missing protocol scheme"#.to_string(),
            r#""cache_object:foo/bar" is not a valid URL: parse "cache_object:foo/bar": first path segment in URL cannot contain colon"#.to_string(),
            r#""http://host:port/" is not a valid URL: parse "http://host:port/": invalid port ":port" after host"#.to_string(),
            r#""http://host/%zz" is not a valid URL: parse "http://host/%zz": invalid URL escape "%zz""#.to_string(),
            r#""http://h|st/" is not a valid URL: parse "http://h|st/": invalid character "|" in host name"#.to_string(),
            r#""http://[::1/" is not a valid URL: parse "http://[::1/": missing ']' in host"#.to_string(),
            r#""http://x[::1]/" is not a valid URL: parse "http://x[::1]/": invalid IP-literal"#.to_string(),
            r#""http://[12345::]/" is not a valid URL: parse "http://[12345::]/": invalid host: ParseAddr("12345::"): each group must have 4 or less digits (at "12345::")"#.to_string(),
            r#""http://us er@host/" is not a valid URL: parse "http://us er@host/": net/url: invalid userinfo"#.to_string(),
            // `%q` renders the ill-formed byte as `\xff`; a U+FFFD would have
            // printed `\xef\xbf\xbd`, three escapes where Go writes one.
            r#""http://example.com/\x7f\xff" is not a valid URL: parse "http://example.com/\x7f\xff": net/url: invalid control character in URL"#.to_string(),
        ],
    );
}

#[test]
fn sa1007_allows_valid_urls() {
    let dir = support::testdata("sa1007");
    let url_stub = dir.join("stub/net/url/url.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1007/ok",
        &dir.join("ok.go"),
        &[("net/url", &url_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1007::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1014_flags_non_pointer_unmarshal_targets() {
    let dir = support::testdata("sa1014");
    let json_stub = dir.join("stub/encoding/json/json.go");
    let io_stub = dir.join("stub/io/io.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1014",
        &dir.join("bad.go"),
        &[("encoding/json", &json_stub), ("io", &io_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1014::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("expects to unmarshal into a pointer"))
    );
}

#[test]
fn sa1014_allows_pointer_unmarshal_targets() {
    let dir = support::testdata("sa1014");
    let json_stub = dir.join("stub/encoding/json/json.go");
    let io_stub = dir.join("stub/io/io.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1014/ok",
        &dir.join("ok.go"),
        &[("encoding/json", &json_stub), ("io", &io_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1014::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1016_flags_untrappable_signals() {
    let dir = support::testdata("sa1016");
    let os_stub = dir.join("stub/os/os.go");
    let signal_stub = dir.join("stub/os/signal/signal.go");
    let syscall_stub = dir.join("stub/syscall/syscall.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1016",
        &dir.join("bad.go"),
        &[
            ("os", &os_stub),
            ("os/signal", &signal_stub),
            ("syscall", &syscall_stub),
        ],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1016::analyzer(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("cannot be trapped")));
}

#[test]
fn sa1016_allows_trappable_signals() {
    let dir = support::testdata("sa1016");
    let os_stub = dir.join("stub/os/os.go");
    let signal_stub = dir.join("stub/os/signal/signal.go");
    let syscall_stub = dir.join("stub/syscall/syscall.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1016/ok",
        &dir.join("ok.go"),
        &[
            ("os", &os_stub),
            ("os/signal", &signal_stub),
            ("syscall", &syscall_stub),
        ],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1016::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1021_flags_bytes_equal_on_net_ip() {
    let dir = support::testdata("sa1021");
    let bytes_stub = dir.join("stub/bytes/bytes.go");
    let net_stub = dir.join("stub/net/ip.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1021",
        &dir.join("bad.go"),
        &[("bytes", &bytes_stub), ("net", &net_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1021::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("net.IP.Equal"));
}

#[test]
fn sa1021_allows_other_byte_comparisons() {
    let dir = support::testdata("sa1021");
    let bytes_stub = dir.join("stub/bytes/bytes.go");
    let net_stub = dir.join("stub/net/ip.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1021/ok",
        &dir.join("ok.go"),
        &[("bytes", &bytes_stub), ("net", &net_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1021::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1028_flags_non_slice_sort_slice_calls() {
    let dir = support::testdata("sa1028");
    let sort_stub = dir.join("stub/sort/sort.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1028",
        &dir.join("bad.go"),
        &[("sort", &sort_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1028::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("must only be called on slices")));
}

#[test]
fn sa1028_allows_slice_sort_slice_calls() {
    let dir = support::testdata("sa1028");
    let sort_stub = dir.join("stub/sort/sort.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1028/ok",
        &dir.join("ok.go"),
        &[("sort", &sort_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1028::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1029_flags_bad_context_keys() {
    let dir = support::testdata("sa1029");
    let ctx_stub = dir.join("stub/context/context.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1029",
        &dir.join("bad.go"),
        &[("context", &ctx_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1029::analyzer(), &pkg);
    assert!(messages.len() >= 4, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("built-in type string")));
    assert!(messages.iter().any(|m| m.contains("not comparable")));
    assert!(messages.iter().any(|m| m.contains("empty anonymous struct")));
}

#[test]
fn sa1029_allows_custom_context_keys() {
    let dir = support::testdata("sa1029");
    let ctx_stub = dir.join("stub/context/context.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1029/ok",
        &dir.join("ok.go"),
        &[("context", &ctx_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1029::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1010_flags_findall_with_zero_count() {
    let dir = support::testdata("sa1010");
    let regexp_stub = dir.join("stub/regexp/regexp.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1010",
        &dir.join("bad.go"),
        &[("regexp", &regexp_stub)],
    );
    support::assert_well_typed(&pkg);

    let messages = support::run_analyzer(sa1010::analyzer(), &pkg);
    assert!(messages.len() >= 2, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("FindAll method with n == 0"))
    );
}

#[test]
fn sa1010_allows_nonzero_findall_count() {
    let dir = support::testdata("sa1010");
    let regexp_stub = dir.join("stub/regexp/regexp.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1010/ok",
        &dir.join("ok.go"),
        &[("regexp", &regexp_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1010::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1011_flags_invalid_utf8_cutsets() {
    let dir = support::testdata("sa1011");
    let strings_stub = dir.join("stub/strings/strings.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1011",
        &dir.join("bad.go"),
        &[("strings", &strings_stub)],
    );
    support::assert_well_typed(&pkg);

    // One per call in bad.go. This used to be `>= 2` behind an `#[ignore]`:
    // string constants were held as Rust text, so "is this valid UTF-8?" was
    // answered by the representation rather than by the program.
    let messages = support::run_analyzer(sa1011::analyzer(), &pkg);
    assert_eq!(messages.len(), 6, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("not a valid UTF-8 encoded string"))
    );
}

#[test]
fn sa1011_allows_valid_utf8_cutsets() {
    let dir = support::testdata("sa1011");
    let strings_stub = dir.join("stub/strings/strings.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1011/ok",
        &dir.join("ok.go"),
        &[("strings", &strings_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1011::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1020_flags_invalid_listen_addresses() {
    let dir = support::testdata("sa1020");
    let http_stub = dir.join("stub/net/http/http.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1020",
        &dir.join("bad.go"),
        &[("net/http", &http_stub)],
    );
    support::assert_well_typed(&pkg);

    let messages = support::run_analyzer(sa1020::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("invalid port or service name in host:port pair"))
    );
}

#[test]
fn sa1020_allows_valid_listen_addresses() {
    let dir = support::testdata("sa1020");
    let http_stub = dir.join("stub/net/http/http.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1020/ok",
        &dir.join("ok.go"),
        &[("net/http", &http_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1020::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1030_flags_invalid_strconv_arguments() {
    let dir = support::testdata("sa1030");
    let strconv_stub = dir.join("stub/strconv/strconv.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1030",
        &dir.join("bad.go"),
        &[("strconv", &strconv_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1030::analyzer(), &pkg);
    assert!(messages.len() >= 4, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("'base'")));
    assert!(messages.iter().any(|m| m.contains("'bitSize'")));
}

#[test]
fn sa1030_allows_valid_strconv_arguments() {
    let dir = support::testdata("sa1030");
    let strconv_stub = dir.join("stub/strconv/strconv.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1030/ok",
        &dir.join("ok.go"),
        &[("strconv", &strconv_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1030::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1003_flags_unsupported_binary_write_types() {
    let dir = support::testdata("sa1003");
    let binary_stub = dir.join("stub/encoding/binary/binary.go");
    let io_stub = dir.join("stub/io/io.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1003",
        &dir.join("bad.go"),
        &[("encoding/binary", &binary_stub), ("io", &io_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1003::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("cannot be used with binary.Write"))
    );
}

#[test]
fn sa1003_allows_supported_binary_write_types() {
    let dir = support::testdata("sa1003");
    let binary_stub = dir.join("stub/encoding/binary/binary.go");
    let io_stub = dir.join("stub/io/io.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1003/ok",
        &dir.join("ok.go"),
        &[("encoding/binary", &binary_stub), ("io", &io_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1003::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1032_flags_swapped_errors_is_arguments() {
    let dir = support::testdata("sa1032");
    let errors_stub = dir.join("stub/errors/errors.go");
    let io_stub = dir.join("stub/io/io.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1032",
        &dir.join("bad.go"),
        &[("errors", &errors_stub), ("io", &io_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1032::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("wrong order"));
}

#[test]
fn sa1032_allows_correct_errors_is_arguments() {
    let dir = support::testdata("sa1032");
    let errors_stub = dir.join("stub/errors/errors.go");
    let io_stub = dir.join("stub/io/io.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1032/ok",
        &dir.join("ok.go"),
        &[("errors", &errors_stub), ("io", &io_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1032::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1027_flags_misaligned_atomic_fields_on_32bit() {
    let dir = support::testdata("sa1027");
    let atomic_stub = dir.join("stub/sync/atomic/atomic.go");
    let sizes = sizes_for("gc", "386").expect("386 sizes");
    let pkg = support::typecheck_with_deps_and_sizes(
        "example.com/staticcheck/sa1027",
        &dir.join("bad.go"),
        &[("sync/atomic", &atomic_stub)],
        Some(sizes),
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1027::analyzer(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("non 64-bit aligned field C"))
    );
}

#[test]
fn sa1027_skips_on_64bit_platform() {
    let dir = support::testdata("sa1027");
    let atomic_stub = dir.join("stub/sync/atomic/atomic.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1027",
        &dir.join("bad.go"),
        &[("sync/atomic", &atomic_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1027::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1027_allows_aligned_atomic_fields_on_32bit() {
    let dir = support::testdata("sa1027");
    let atomic_stub = dir.join("stub/sync/atomic/atomic.go");
    let sizes = sizes_for("gc", "386").expect("386 sizes");
    let pkg = support::typecheck_with_deps_and_sizes(
        "example.com/staticcheck/sa1027/ok",
        &dir.join("ok.go"),
        &[("sync/atomic", &atomic_stub)],
        Some(sizes),
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1027::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1031_flags_overlapping_encode_slices() {
    let dir = support::testdata("sa1031");
    let hex_stub = dir.join("stub/encoding/hex/hex.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1031",
        &dir.join("bad.go"),
        &[("encoding/hex", &hex_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1031::analyzer(), &pkg);
    assert_eq!(messages.len(), 8, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("overlapping dst and src")));
}

#[test]
fn sa1031_allows_non_overlapping_encode_slices() {
    let dir = support::testdata("sa1031");
    let hex_stub = dir.join("stub/encoding/hex/hex.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1031/ok",
        &dir.join("ok.go"),
        &[("encoding/hex", &hex_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1031::analyzer(), &pkg).is_empty());
}

#[test]
fn s1008_flags_redundant_if_return_patterns() {
    let dir = support::testdata("s1008");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1008");
    support::assert_well_typed(&pkg);

    let messages = support::run_analyzer(s1008::analyzer(), &pkg);
    assert!(messages.len() >= 4, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("return x'")));
    assert!(messages.iter().any(|m| m.contains("return !fn()")));
    assert!(messages.iter().any(|m| m.contains("return x <= 0")));
    assert!(messages.iter().any(|m| m.contains("return len(x) == 0")));
}

#[test]
fn s1008_allows_valid_returns() {
    let dir = support::testdata("s1008");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1008/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1008::analyzer(), &pkg).is_empty());
}

#[test]
fn s1000_flags_bad_patterns() {
    let dir = support::testdata("s1000");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1000");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1000::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("simple channel send")));
}

#[test]
fn s1000_allows_ok_patterns() {
    let dir = support::testdata("s1000");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1000/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1000::analyzer(), &pkg).is_empty());
}

#[test]
fn s1001_flags_bad_patterns() {
    let dir = support::testdata("s1001");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1001");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1001::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("copy(to, from)")));
}

#[test]
fn s1001_allows_ok_patterns() {
    let dir = support::testdata("s1001");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1001/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1001::analyzer(), &pkg).is_empty());
}

#[test]
fn s1004_flags_bad_patterns() {
    let dir = support::testdata("s1004");
    let bytes_stub = dir.join("stub/bytes/bytes.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1004",
        &dir.join("bad.go"),
        &[("bytes", &bytes_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1004::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("bytes.Equal")));
}

#[test]
fn s1004_allows_ok_patterns() {
    let dir = support::testdata("s1004");
    let bytes_stub = dir.join("stub/bytes/bytes.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1004/ok",
        &dir.join("ok.go"),
        &[("bytes", &bytes_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1004::analyzer(), &pkg).is_empty());
}

#[test]
fn s1005_flags_bad_patterns() {
    let dir = support::testdata("s1005");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1005");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1005::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("blank identifier")));
}

#[test]
fn s1005_allows_ok_patterns() {
    let dir = support::testdata("s1005");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1005/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1005::analyzer(), &pkg).is_empty());
}

#[test]
fn s1007_flags_bad_patterns() {
    let dir = support::testdata("s1007");
    let regexp_stub = dir.join("stub/regexp/regexp.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1007",
        &dir.join("bad.go"),
        &[("regexp", &regexp_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1007::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("raw string")));
}

#[test]
fn s1007_allows_ok_patterns() {
    let dir = support::testdata("s1007");
    let regexp_stub = dir.join("stub/regexp/regexp.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1007/ok",
        &dir.join("ok.go"),
        &[("regexp", &regexp_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1007::analyzer(), &pkg).is_empty());
}

#[test]
fn s1010_flags_bad_patterns() {
    let dir = support::testdata("s1010");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1010");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1010::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("omit second index")));
}

#[test]
fn s1010_allows_ok_patterns() {
    let dir = support::testdata("s1010");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1010/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1010::analyzer(), &pkg).is_empty());
}

#[test]
fn s1011_flags_bad_patterns() {
    let dir = support::testdata("s1011");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1011");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1011::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("x = append(x, y...)")));
}

#[test]
fn s1011_allows_ok_patterns() {
    let dir = support::testdata("s1011");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1011/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1011::analyzer(), &pkg).is_empty());
}

#[test]
fn s1012_flags_bad_patterns() {
    let dir = support::testdata("s1012");
    let time_stub = dir.join("stub/time/time.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1012",
        &dir.join("bad.go"),
        &[("time", &time_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1012::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("time.Since")));
}

#[test]
fn s1012_allows_ok_patterns() {
    let dir = support::testdata("s1012");
    let time_stub = dir.join("stub/time/time.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1012/ok",
        &dir.join("ok.go"),
        &[("time", &time_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1012::analyzer(), &pkg).is_empty());
}

#[test]
fn s1016_flags_bad_patterns() {
    let dir = support::testdata("s1016");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1016");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1016::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("should convert")));
}

#[test]
fn s1016_allows_ok_patterns() {
    let dir = support::testdata("s1016");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1016/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1016::analyzer(), &pkg).is_empty());
}

#[test]
fn s1018_flags_bad_patterns() {
    let dir = support::testdata("s1018");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1018");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1018::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("copy()")));
}

#[test]
fn s1018_allows_ok_patterns() {
    let dir = support::testdata("s1018");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1018/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1018::analyzer(), &pkg).is_empty());
}

#[test]
fn s1019_flags_bad_patterns() {
    let dir = support::testdata("s1019");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1019");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1019::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    // Upstream renders the suggested call from the source, not a `T` placeholder.
    assert!(messages.iter().any(|m| m.contains("make(chan int)")), "{messages:?}");
}

#[test]
fn s1019_allows_ok_patterns() {
    let dir = support::testdata("s1019");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1019/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1019::analyzer(), &pkg).is_empty());
}

#[test]
fn s1020_flags_bad_patterns() {
    let dir = support::testdata("s1020");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1020");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1020::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("can't be nil")));
}

#[test]
fn s1020_allows_ok_patterns() {
    let dir = support::testdata("s1020");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1020/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1020::analyzer(), &pkg).is_empty());
}

#[test]
fn s1024_flags_bad_patterns() {
    let dir = support::testdata("s1024");
    let time_stub = dir.join("stub/time/time.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1024",
        &dir.join("bad.go"),
        &[("time", &time_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1024::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("time.Until")));
}

#[test]
fn s1024_allows_ok_patterns() {
    let dir = support::testdata("s1024");
    let time_stub = dir.join("stub/time/time.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1024/ok",
        &dir.join("ok.go"),
        &[("time", &time_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1024::analyzer(), &pkg).is_empty());
}

#[test]
fn s1025_flags_bad_patterns() {
    let dir = support::testdata("s1025");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1025",
        &dir.join("bad.go"),
        &[("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1025::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("already a string")));
}

#[test]
fn s1025_allows_ok_patterns() {
    let dir = support::testdata("s1025");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1025/ok",
        &dir.join("ok.go"),
        &[("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1025::analyzer(), &pkg).is_empty());
}

#[test]
fn s1028_flags_bad_patterns() {
    let dir = support::testdata("s1028");
    let errors_stub = dir.join("stub/errors/errors.go");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1028",
        &dir.join("bad.go"),
        &[("errors", &errors_stub), ("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1028::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("fmt.Errorf")));
}

#[test]
fn s1028_allows_ok_patterns() {
    let dir = support::testdata("s1028");
    let errors_stub = dir.join("stub/errors/errors.go");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1028/ok",
        &dir.join("ok.go"),
        &[("errors", &errors_stub), ("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1028::analyzer(), &pkg).is_empty());
}

#[test]
fn s1030_flags_bad_patterns() {
    let dir = support::testdata("s1030");
    let bytes_stub = dir.join("stub/bytes/buffer.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1030",
        &dir.join("bad.go"),
        &[("bytes", &bytes_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1030::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("buf.String()")));
}

#[test]
fn s1030_allows_ok_patterns() {
    let dir = support::testdata("s1030");
    let bytes_stub = dir.join("stub/bytes/buffer.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1030/ok",
        &dir.join("ok.go"),
        &[("bytes", &bytes_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1030::analyzer(), &pkg).is_empty());
}

#[test]
fn s1031_flags_bad_patterns() {
    let dir = support::testdata("s1031");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1031");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1031::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("nil check around range")));
}

#[test]
fn s1031_allows_ok_patterns() {
    let dir = support::testdata("s1031");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1031/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1031::analyzer(), &pkg).is_empty());
}

#[test]
fn s1033_flags_bad_patterns() {
    let dir = support::testdata("s1033");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1033");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1033::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("guard around call to delete")));
}

#[test]
fn s1033_allows_ok_patterns() {
    let dir = support::testdata("s1033");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1033/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1033::analyzer(), &pkg).is_empty());
}

#[test]
fn s1034_flags_bad_patterns() {
    let dir = support::testdata("s1034");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1034");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1034::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("type assertion to a variable")));
}

#[test]
fn s1034_allows_ok_patterns() {
    let dir = support::testdata("s1034");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1034/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1034::analyzer(), &pkg).is_empty());
}

#[test]
fn s1035_flags_bad_patterns() {
    let dir = support::testdata("s1035");
    let net_http_stub = dir.join("stub/net/http/http.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1035",
        &dir.join("bad.go"),
        &[("net/http", &net_http_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1035::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("CanonicalHeaderKey")));
}

#[test]
fn s1035_allows_ok_patterns() {
    let dir = support::testdata("s1035");
    let net_http_stub = dir.join("stub/net/http/http.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1035/ok",
        &dir.join("ok.go"),
        &[("net/http", &net_http_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1035::analyzer(), &pkg).is_empty());
}

#[test]
fn s1036_flags_bad_patterns() {
    let dir = support::testdata("s1036");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1036");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1036::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("guard around map access")));
}

#[test]
fn s1036_allows_ok_patterns() {
    let dir = support::testdata("s1036");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1036/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1036::analyzer(), &pkg).is_empty());
}

#[test]
fn s1037_flags_bad_patterns() {
    let dir = support::testdata("s1037");
    let time_stub = dir.join("stub/time/time.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1037",
        &dir.join("bad.go"),
        &[("time", &time_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1037::analyzer(), &pkg);
    // Both clauses of `bad.go`, and nothing else: `ok.go` holds the four
    // shapes upstream declines, three of which guff used to report.
    assert_eq!(
        messages,
        vec![
            "should use time.Sleep instead of elaborate way of sleeping",
            "should use time.Sleep instead of elaborate way of sleeping",
        ],
        "{messages:?}"
    );
}

#[test]
fn s1037_allows_ok_patterns() {
    let dir = support::testdata("s1037");
    let time_stub = dir.join("stub/time/time.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1037/ok",
        &dir.join("ok.go"),
        &[("time", &time_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1037::analyzer(), &pkg).is_empty());
}

#[test]
fn s1038_flags_bad_patterns() {
    let dir = support::testdata("s1038");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1038",
        &dir.join("bad.go"),
        &[("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1038::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("fmt.Printf")));
}

#[test]
fn s1038_allows_ok_patterns() {
    let dir = support::testdata("s1038");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1038/ok",
        &dir.join("ok.go"),
        &[("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1038::analyzer(), &pkg).is_empty());
}

#[test]
fn s1039_flags_bad_patterns() {
    let dir = support::testdata("s1039");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1039",
        &dir.join("bad.go"),
        &[("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1039::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("unnecessary use of fmt.Sprint")));
}

#[test]
fn s1039_allows_ok_patterns() {
    let dir = support::testdata("s1039");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/s1039/ok",
        &dir.join("ok.go"),
        &[("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1039::analyzer(), &pkg).is_empty());
}

#[test]
fn s1040_flags_bad_patterns() {
    let dir = support::testdata("s1040");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1040");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(s1040::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("type assertion to the same type")));
}

#[test]
fn s1040_allows_ok_patterns() {
    let dir = support::testdata("s1040");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1040/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(s1040::analyzer(), &pkg).is_empty());
}

#[test]
fn sa1001_flags_invalid_template() {
    let dir = support::testdata("sa1001");
    let tmpl_stub = dir.join("stub/text/template/template.go");
    let html_stub = dir.join("stub/html/template/template.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1001",
        &dir.join("bad.go"),
        &[
            ("text/template", &tmpl_stub),
            ("html/template", &html_stub),
        ],
    );
    support::assert_well_typed(&pkg);
    // Pin the whole message: `contains("unexpected")` passed for years against
    // a brace counter that got the wording, the position and the reported set
    // wrong. Expectations are golangci-lint 2.12.2's output (compat/golden),
    // and the parser behind them is gated by tests/gostd_template.rs.
    let messages = support::run_analyzer(sa1001::analyzer(), &pkg);
    assert_eq!(
        messages,
        vec![
            r#"template: :1: bad character U+007D '}'"#.to_string(),
            r#"template: :1: bad character U+002B '+'"#.to_string(),
            r#"template: :1: unexpected right paren"#.to_string(),
            r#"template: :1: unexpected EOF"#.to_string(),
            r#"template: :1: unexpected {{end}}"#.to_string(),
            r#"template: :1: unexpected "," in command"#.to_string(),
            r#"template: :1: unexpected . after term "true""#.to_string(),
            r#"template: :1: unexpected "1" in template clause"#.to_string(),
            r#"template: :1: unexpected ".3" in operand"#.to_string(),
            r#"template: :1: unexpected {{else}} in define clause"#.to_string(),
            r#"template: :2: unexpected {{end}}"#.to_string(),
            r#"template: :1: unexpected {{end}}"#.to_string(),
        ],
    );
}

#[test]
fn sa1001_allows_valid_template() {
    let dir = support::testdata("sa1001");
    let tmpl_stub = dir.join("stub/text/template/template.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1001/ok",
        &dir.join("ok.go"),
        &[("text/template", &tmpl_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa1001::analyzer(), &pkg).is_empty());
}

#[test]
fn sa2000_flags_waitgroup_add_in_goroutine() {
    let dir = support::testdata("sa2000");
    let sync_stub = dir.join("stub/sync/sync.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa2000",
        &dir.join("bad.go"),
        &[("sync", &sync_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa2000::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("before starting the goroutine"));
}

#[test]
fn sa2000_allows_waitgroup_add_before_goroutine() {
    let dir = support::testdata("sa2000");
    let sync_stub = dir.join("stub/sync/sync.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa2000/ok",
        &dir.join("ok.go"),
        &[("sync", &sync_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa2000::analyzer(), &pkg).is_empty());
}

#[test]
fn sa2001_flags_empty_critical_section() {
    let dir = support::testdata("sa2001");
    let sync_stub = dir.join("stub/sync/sync.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa2001",
        &dir.join("bad.go"),
        &[("sync", &sync_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa2001::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("empty critical section"));
}

#[test]
fn sa2001_allows_deferred_unlock() {
    let dir = support::testdata("sa2001");
    let sync_stub = dir.join("stub/sync/sync.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa2001/ok",
        &dir.join("ok.go"),
        &[("sync", &sync_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa2001::analyzer(), &pkg).is_empty());
}

#[test]
fn sa2002_flags_fatal_in_goroutine() {
    let dir = support::testdata("sa2002");
    let testing_stub = dir.join("stub/testing/testing.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa2002",
        &dir.join("bad.go"),
        &[("testing", &testing_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa2002::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("goroutine calls T.Fatal"));
}

#[test]
fn sa2002_allows_fatal_in_test_goroutine() {
    let dir = support::testdata("sa2002");
    let testing_stub = dir.join("stub/testing/testing.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa2002/ok",
        &dir.join("ok.go"),
        &[("testing", &testing_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa2002::analyzer(), &pkg).is_empty());
}

#[test]
fn sa2003_flags_deferred_lock_after_lock() {
    let dir = support::testdata("sa2003");
    let sync_stub = dir.join("stub/sync/sync.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa2003",
        &dir.join("bad.go"),
        &[("sync", &sync_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa2003::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("defer Unlock"));
}

#[test]
fn sa2003_allows_deferred_unlock() {
    let dir = support::testdata("sa2003");
    let sync_stub = dir.join("stub/sync/sync.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa2003/ok",
        &dir.join("ok.go"),
        &[("sync", &sync_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa2003::analyzer(), &pkg).is_empty());
}

#[test]
fn sa3000_flags_testmain_without_exit() {
    let dir = support::testdata("sa3000");
    let testing_stub = dir.join("stub/testing/testing.go");
    let os_stub = dir.join("stub/os/os.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa3000",
        &dir.join("bad.go"),
        &[("testing", &testing_stub), ("os", &os_stub)],
    );
    support::assert_well_typed(&pkg);
    let pkg = support::with_go_version(pkg, "1.14");
    let messages = support::run_analyzer(sa3000::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("os.Exit"));
}

#[test]
fn sa3000_allows_testmain_with_exit() {
    let dir = support::testdata("sa3000");
    let testing_stub = dir.join("stub/testing/testing.go");
    let os_stub = dir.join("stub/os/os.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa3000/ok",
        &dir.join("ok.go"),
        &[("testing", &testing_stub), ("os", &os_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa3000::analyzer(), &pkg).is_empty());
}

#[test]
fn sa3001_flags_benchmark_n_assignment() {
    let dir = support::testdata("sa3001");
    let testing_stub = dir.join("stub/testing/testing.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa3001",
        &dir.join("bad.go"),
        &[("testing", &testing_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa3001::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("should not assign to b.N"));
}

#[test]
fn sa3001_allows_reading_benchmark_n() {
    let dir = support::testdata("sa3001");
    let testing_stub = dir.join("stub/testing/testing.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa3001/ok",
        &dir.join("ok.go"),
        &[("testing", &testing_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa3001::analyzer(), &pkg).is_empty());
}

fn typecheck_rule(rule: &str, file: &str) -> std::sync::Arc<guff_packages::Package> {
    let dir = support::testdata(rule);
    let deps_owned = support::collect_stubs(&dir);
    let deps: Vec<(&str, &std::path::Path)> = deps_owned
        .iter()
        .map(|(p, path)| (p.as_str(), path.as_path()))
        .collect();
    support::typecheck_with_deps(
        &format!("example.com/staticcheck/{rule}/{file}"),
        &dir.join(file),
        &deps,
    )
}

macro_rules! sa_check {
    ($rule:ident, $bad_test:ident, $ok_test:ident, $substr:expr) => {
        #[test]
        fn $bad_test() {
            let pkg = typecheck_rule(stringify!($rule), "bad.go");
            support::assert_well_typed(&pkg);
            let messages = support::run_analyzer($rule::analyzer(), &pkg);
            assert!(!messages.is_empty(), "{messages:?}");
            assert!(messages[0].contains($substr), "{messages:?}");
        }

        #[test]
        fn $ok_test() {
            let pkg = typecheck_rule(stringify!($rule), "ok.go");
            support::assert_well_typed(&pkg);
            assert!(support::run_analyzer($rule::analyzer(), &pkg).is_empty());
        }
    };
}

macro_rules! sa_check_bad_ok {
    ($rule:ident, $bad_test:ident, $ok_test:ident) => {
        #[test]
        fn $bad_test() {
            let pkg = typecheck_rule(stringify!($rule), "bad.go");
            support::assert_well_typed(&pkg);
            let messages = support::run_analyzer($rule::analyzer(), &pkg);
            assert!(!messages.is_empty(), "{messages:?}");
        }

        #[test]
        fn $ok_test() {
            let pkg = typecheck_rule(stringify!($rule), "ok.go");
            support::assert_well_typed(&pkg);
            assert!(support::run_analyzer($rule::analyzer(), &pkg).is_empty());
        }
    };
}

sa_check!(sa1015, sa1015_flags_leaky_time_tick, sa1015_allows_endless_time_tick, "leaks the underlying ticker");
sa_check!(sa1019, sa1019_flags_deprecated_use, sa1019_allows_clean_code, "deprecated");

#[test]
fn sa1019_flags_a_deprecated_struct_field_of_an_imported_type() {
    // Reported by the OSS tier the day controller-runtime's ill-typed packages
    // went to zero: five `//nolint:staticcheck` directives turned up "unused"
    // because the finding they suppress upstream was never made here.
    // `o.Fine` is the control — a live field of the same struct must stay quiet.
    let pkg = typecheck_rule("sa1019", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1019::analyzer(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("o.Old is deprecated")),
        "deprecated struct field not reported: {messages:?}"
    );
    assert!(
        messages.iter().all(|m| !m.contains("o.Fine is deprecated")),
        "a live field of the same struct must stay quiet: {messages:?}"
    );
}
sa_check!(sa1023, sa1023_flags_writer_buffer_modified, sa1023_allows_readonly_write, "must not modify the provided buffer");
sa_check!(sa1025, sa1025_flags_timer_reset_return, sa1025_allows_timer_reset_without_drain, "Reset's return value");

sa_check_bad_ok!(sa4000, sa4000_flags_bad_cases, sa4000_allows_ok_cases);
sa_check_bad_ok!(sa4001, sa4001_flags_bad_cases, sa4001_allows_ok_cases);
sa_check_bad_ok!(sa4003, sa4003_flags_bad_cases, sa4003_allows_ok_cases);
sa_check_bad_ok!(sa4004, sa4004_flags_bad_cases, sa4004_allows_ok_cases);
sa_check_bad_ok!(sa4005, sa4005_flags_bad_cases, sa4005_allows_ok_cases);
#[test]
fn sa4006_flags_bad_cases() {
    let pkg = typecheck_rule("sa4006", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa4006::analyzer(), &pkg);
    assert_eq!(messages.len(), 4, "{messages:?}");
}

#[test]
fn sa4006_allows_ok_cases() {
    let pkg = typecheck_rule("sa4006", "ok.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa4006::analyzer(), &pkg);
    // golangci-lint 2.12.2 reports nothing here, and neither does guff since
    // `MakeInterface` gained its operand: `i = n` boxes `n`, so `n` has a
    // referrer and no longer looks unused.
    assert!(messages.is_empty(), "{messages:?}");
}
sa_check_bad_ok!(sa4008, sa4008_flags_bad_cases, sa4008_allows_ok_cases);
sa_check_bad_ok!(sa4009, sa4009_flags_bad_cases, sa4009_allows_ok_cases);
sa_check_bad_ok!(sa4010, sa4010_flags_bad_cases, sa4010_allows_ok_cases);
sa_check_bad_ok!(sa4011, sa4011_flags_bad_cases, sa4011_allows_ok_cases);
sa_check_bad_ok!(sa4012, sa4012_flags_bad_cases, sa4012_allows_ok_cases);
sa_check_bad_ok!(sa4013, sa4013_flags_bad_cases, sa4013_allows_ok_cases);
sa_check_bad_ok!(sa4014, sa4014_flags_bad_cases, sa4014_allows_ok_cases);
sa_check_bad_ok!(sa4015, sa4015_flags_bad_cases, sa4015_allows_ok_cases);
sa_check_bad_ok!(sa4016, sa4016_flags_bad_cases, sa4016_allows_ok_cases);
sa_check_bad_ok!(sa4017, sa4017_flags_bad_cases, sa4017_allows_ok_cases);
sa_check_bad_ok!(sa4018, sa4018_flags_bad_cases, sa4018_allows_ok_cases);
sa_check_bad_ok!(sa4019, sa4019_flags_bad_cases, sa4019_allows_ok_cases);
sa_check_bad_ok!(sa4020, sa4020_flags_bad_cases, sa4020_allows_ok_cases);
sa_check_bad_ok!(sa4021, sa4021_flags_bad_cases, sa4021_allows_ok_cases);
sa_check_bad_ok!(sa4022, sa4022_flags_bad_cases, sa4022_allows_ok_cases);
sa_check_bad_ok!(sa4023, sa4023_flags_bad_cases, sa4023_allows_ok_cases);
sa_check_bad_ok!(sa4024, sa4024_flags_bad_cases, sa4024_allows_ok_cases);
sa_check_bad_ok!(sa4025, sa4025_flags_bad_cases, sa4025_allows_ok_cases);
sa_check_bad_ok!(sa4026, sa4026_flags_bad_cases, sa4026_allows_ok_cases);
sa_check_bad_ok!(sa4027, sa4027_flags_bad_cases, sa4027_allows_ok_cases);
sa_check_bad_ok!(sa4028, sa4028_flags_bad_cases, sa4028_allows_ok_cases);
sa_check_bad_ok!(sa4029, sa4029_flags_bad_cases, sa4029_allows_ok_cases);
sa_check_bad_ok!(sa4030, sa4030_flags_bad_cases, sa4030_allows_ok_cases);
sa_check_bad_ok!(sa4031, sa4031_flags_bad_cases, sa4031_allows_ok_cases);
sa_check_bad_ok!(sa4032, sa4032_flags_bad_cases, sa4032_allows_ok_cases);

sa_check!(sa5000, sa5000_flags_nil_map, sa5000_allows_initialized_map, "assignment to nil map");
sa_check!(sa5002, sa5002_flags_empty_loop, sa5002_allows_nonempty_loop, "spin");
sa_check!(sa5003, sa5003_flags_defers_in_infinite_loop, sa5003_allows_finite_loop, "defers in this infinite loop");
sa_check!(sa5004, sa5004_flags_empty_default_select, sa5004_allows_nonempty_default_select, "empty default case");
sa_check!(sa5005, sa5005_flags_finalizer_self_ref, sa5005_allows_safe_finalizer, "finalizer closes over");
sa_check!(sa5007, sa5007_flags_infinite_recursion, sa5007_allows_terminating_recursion, "infinite recursive call");
sa_check!(sa5008, sa5008_flags_invalid_struct_tag, sa5008_allows_valid_struct_tag, "invalid XML tag");
sa_check!(sa5010, sa5010_flags_impossible_assertion, sa5010_allows_possible_assertion, "type assertion");
sa_check!(sa5011, sa5011_flags_possible_nil_deref, sa5011_allows_guarded_deref, "possible nil pointer dereference");
sa_check!(sa6003, sa6003_flags_rune_range, sa6003_allows_string_range, "range over string");
sa_check!(sa9001, sa9001_flags_defer_in_channel_range, sa9001_allows_defer_outside_range, "defer");
sa_check!(sa9003, sa9003_flags_empty_branch, sa9003_allows_nonempty_branch, "empty branch");
sa_check!(sa9004, sa9004_flags_mixed_const_types, sa9004_allows_uniform_const_types, "only the first constant");
sa_check!(sa9006, sa9006_flags_fixed_shift, sa9006_allows_variable_shift, "shift");
sa_check!(sa9009, sa9009_flags_ineffectual_directive, sa9009_allows_valid_directive, "go:");

/// `if a || b { … }` renames the pointer, in both directions.
///
/// honnef's IR is SSI: the `err != nil` branch decides whether the `p == nil`
/// check is reached at all, so the check's operand is a sigma and the block
/// below the join reads a phi merging both edges. SA5011 is pure value
/// identity, so it cannot match across either — whatever the branch body does,
/// and whichever side the deref is on. coredns writes fifteen of these in
/// `test/wildcard_test.go` and they were the whole of its staticcheck diff.
#[test]
fn sa5011_or_guard_renames_the_pointer() {
    let pkg = typecheck_rule("sa5011", "ok.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa5011::analyzer(), &pkg);
    assert!(
        messages.is_empty(),
        "every OR-guarded shape in ok.go is silent upstream: {messages:?}"
    );

    // The single-check spelling of the deref-first shape is still a finding.
    let bad = typecheck_rule("sa5011", "bad.go");
    let bad_messages = support::run_analyzer(sa5011::analyzer(), &bad);
    assert!(
        bad_messages.len() == 2,
        "bad.go keeps both of its findings: {bad_messages:?}"
    );
}

/// `ctrlflow` proves `(*testing.T).Fatal` never returns, so the code below the
/// `if` is entered only from the non-nil side and reads a sigma. The receiver
/// decides whether the call aborts, not the enclosing function — nats-server
/// dereferences inside a `func(k, v any) bool` callback after a `t.Fatalf` on a
/// captured concrete `*testing.T`.
#[test]
fn sa5011_reads_the_abort_from_the_receiver() {
    let dir = support::testdata("sa5011");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa5011/testing_abort",
        &dir.join("testing_abort.go"),
        &[("testing", &dir.join("stub/testing/testing.go"))],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa5011::analyzer(), &pkg);
    assert!(
        messages.is_empty(),
        "an abort on a concrete *testing.T guards both shapes: {messages:?}"
    );
}

#[test]
fn sa5001_flags_defer_before_error_check() {
    let pkg = typecheck_rule("sa5001", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa5001::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("before deferring"), "{messages:?}");
}

#[test]
fn sa5001_allows_defer_after_error_check() {
    let pkg = typecheck_rule("sa5001", "ok.go");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa5001::analyzer(), &pkg).is_empty());
}

#[test]
fn sa5009_flags_invalid_printf() {
    let pkg = typecheck_rule("sa5009", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa5009::analyzer(), &pkg);
    // Pin the whole string: asserting only `contains("Printf")` is what let
    // guff report the too-many-args wording ("Printf call needs 2 args but has
    // 0 args") for a too-few-args call for as long as it did.
    assert_eq!(
        messages,
        vec!["Printf format %s reads arg #1, but call has only 0 args"],
        "{messages:?}"
    );
}

#[test]
fn sa5009_allows_valid_printf() {
    let pkg = typecheck_rule("sa5009", "ok.go");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa5009::analyzer(), &pkg).is_empty());
}

#[test]
fn sa5012_flags_odd_new_replacer_slice() {
    let pkg = typecheck_rule("sa5012", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa5012::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("even number"), "{messages:?}");
}

#[test]
fn sa5012_allows_even_new_replacer_slice() {
    let pkg = typecheck_rule("sa5012", "ok.go");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa5012::analyzer(), &pkg).is_empty());
}

#[test]
fn sa6000_flags_regexp_match_in_loop() {
    let pkg = typecheck_rule("sa6000", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa6000::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("regexp.Compile"), "{messages:?}");
}

#[test]
fn sa6000_allows_regexp_compile_in_loop() {
    let pkg = typecheck_rule("sa6000", "ok.go");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa6000::analyzer(), &pkg).is_empty());
}

#[test]
fn sa6001_flags_map_string_key() {
    let pkg = typecheck_rule("sa6001", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa6001::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("string("), "{messages:?}");
}

#[test]
fn sa6001_allows_direct_byte_slice_key() {
    let pkg = typecheck_rule("sa6001", "ok.go");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa6001::analyzer(), &pkg).is_empty());
}

#[test]
fn sa6002_flags_non_pointer_pool_put() {
    let pkg = typecheck_rule("sa6002", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa6002::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("pointer-like"), "{messages:?}");
}

#[test]
fn sa6002_allows_pointer_pool_put() {
    let pkg = typecheck_rule("sa6002", "ok.go");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa6002::analyzer(), &pkg).is_empty());
}

#[test]
fn sa6005_flags_tolower_comparison() {
    let pkg = typecheck_rule("sa6005", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa6005::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("EqualFold"), "{messages:?}");
}

#[test]
fn sa6005_allows_equal_fold_comparison() {
    let pkg = typecheck_rule("sa6005", "ok.go");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa6005::analyzer(), &pkg).is_empty());
}

#[test]
fn sa6006_flags_write_string() {
    let pkg = typecheck_rule("sa6006", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa6006::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("WriteString"), "{messages:?}");
}

#[test]
fn sa6006_allows_write() {
    let pkg = typecheck_rule("sa6006", "ok.go");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa6006::analyzer(), &pkg).is_empty());
}

#[test]
fn sa9002_flags_file_mode_octal() {
    let pkg = typecheck_rule("sa9002", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa9002::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("0644"), "{messages:?}");
}

#[test]
fn sa9002_allows_explicit_octal_file_mode() {
    let pkg = typecheck_rule("sa9002", "ok.go");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa9002::analyzer(), &pkg).is_empty());
}

#[test]
fn sa9005_flags_unexported_struct_marshal() {
    let pkg = typecheck_rule("sa9005", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa9005::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("exported fields"), "{messages:?}");
}

#[test]
fn sa9005_allows_exported_struct_marshal() {
    let pkg = typecheck_rule("sa9005", "ok.go");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa9005::analyzer(), &pkg).is_empty());
}

#[test]
fn sa9007_flags_remove_all_on_system_dir() {
    let pkg = typecheck_rule("sa9007", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa9007::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("RemoveAll"), "{messages:?}");
}

#[test]
fn sa9007_allows_remove_all_on_temp_dir() {
    let pkg = typecheck_rule("sa9007", "ok.go");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa9007::analyzer(), &pkg).is_empty());
}

#[test]
fn sa9008_flags_type_assertion_shadowing() {
    let pkg = typecheck_rule("sa9008", "bad.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa9008::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("failed type assertion"), "{messages:?}");
}

#[test]
fn sa9008_allows_type_assertion_without_shadowing() {
    let pkg = typecheck_rule("sa9008", "ok.go");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(sa9008::analyzer(), &pkg).is_empty());
}

#[test]
fn st1001_flags_dot_imports() {
    let dir = support::testdata("st1001");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1001",
        &dir.join("bad.go"),
        &[("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1001::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("dot imports")));
}

#[test]
fn st1000_flags_missing_package_comment() {
    let dir = support::testdata("st1000");
    let pkg = support::typecheck_file(&dir, "bad_missing.go", "example.com/staticcheck/st1000/missing");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1000::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("at least one file in a package should have a package comment"));
}

#[test]
fn st1000_flags_badly_formed_package_comment() {
    let dir = support::testdata("st1000");
    let pkg = support::typecheck_file(&dir, "bad_form.go", "example.com/staticcheck/st1000/form");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1000::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("package comment should be of the form"));
}

#[test]
fn st1000_allows_well_formed_package_comment() {
    let dir = support::testdata("st1000");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/st1000/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1000::analyzer(), &pkg).is_empty());
}

#[test]
fn st1001_allows_normal_imports() {
    let dir = support::testdata("st1001");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1001/ok",
        &dir.join("ok.go"),
        &[("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1001::analyzer(), &pkg).is_empty());
}

#[test]
fn st1006_flags_bad_receiver_names() {
    let dir = support::testdata("st1006");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/st1006");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1006::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("underscore")));
    assert!(messages.iter().any(|m| m.contains("self") || m.contains("this") || m.contains("identity")));
}

#[test]
fn st1006_allows_good_receiver_names() {
    let dir = support::testdata("st1006");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/st1006/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1006::analyzer(), &pkg).is_empty());
}

#[test]
fn st1012_flags_badly_named_error_vars() {
    let dir = support::testdata("st1012");
    let errors_stub = dir.join("stub/errors/errors.go");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1012",
        &dir.join("bad.go"),
        &[("errors", &errors_stub), ("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1012::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("error var foo")));
    assert!(messages.iter().any(|m| m.contains("error var abc")));
    assert!(messages.iter().any(|m| m.contains("error var wrong")));
}

#[test]
fn st1012_allows_well_named_error_vars() {
    let dir = support::testdata("st1012");
    let errors_stub = dir.join("stub/errors/errors.go");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1012/ok",
        &dir.join("ok.go"),
        &[("errors", &errors_stub), ("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1012::analyzer(), &pkg).is_empty());
}

#[test]
fn st1015_flags_middle_default_case() {
    let dir = support::testdata("st1015");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/st1015");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1015::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("default case should be first or last"));
}

#[test]
fn st1015_allows_first_or_last_default() {
    let dir = support::testdata("st1015");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/st1015/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1015::analyzer(), &pkg).is_empty());
}

#[test]
fn st1011_flags_duration_unit_suffixes() {
    let dir = support::testdata("st1011");
    let time_stub = dir.join("stub/time/time.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1011",
        &dir.join("bad.go"),
        &[("time", &time_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1011::analyzer(), &pkg);
    assert!(messages.len() >= 5, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("BMillis")));
    assert!(messages.iter().any(|m| m.contains("cMS")));
    assert!(messages.iter().any(|m| m.contains("xMS")));
    assert!(messages.iter().any(|m| m.contains("bMS")));
}

#[test]
fn st1011_allows_duration_without_unit_suffix() {
    let dir = support::testdata("st1011");
    let time_stub = dir.join("stub/time/time.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1011/ok",
        &dir.join("ok.go"),
        &[("time", &time_stub)],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1011::analyzer(), &pkg).is_empty());
}

#[test]
fn st1017_flags_yoda_conditions() {
    let dir = support::testdata("st1017");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/st1017");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1017::analyzer(), &pkg);
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("Yoda")));
}

#[test]
fn st1017_allows_non_yoda_conditions() {
    let dir = support::testdata("st1017");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/st1017/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1017::analyzer(), &pkg).is_empty());
}

#[test]
fn st1019_flags_duplicate_imports() {
    let dir = support::testdata("st1019");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1019",
        &dir.join("bad.go"),
        &[
            ("fmt", &dir.join("stub/fmt/fmt.go")),
            ("os", &dir.join("stub/os/os.go")),
            ("net/http/pprof", &dir.join("stub/net/http/pprof/pprof.go")),
            ("strconv", &dir.join("stub/strconv/strconv.go")),
        ],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1019::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("\"fmt\"")));
    assert!(messages.iter().any(|m| m.contains("\"os\"")));
    assert!(messages.iter().any(|m| m.contains("\"strconv\"")));
    assert!(!messages.iter().any(|m| m.contains("pprof")));
}

#[test]
fn st1019_allows_unique_imports() {
    let dir = support::testdata("st1019");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1019/ok",
        &dir.join("ok.go"),
        &[
            ("fmt", &dir.join("stub/fmt/fmt.go")),
            ("os", &dir.join("stub/os/os.go")),
        ],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1019::analyzer(), &pkg).is_empty());
}

#[test]
fn st1013_flags_http_status_magic_numbers() {
    let dir = support::testdata("st1013");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1013",
        &dir.join("bad.go"),
        &[("net/http", &dir.join("stub/net/http/http.go"))],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1013::analyzer(), &pkg);
    assert_eq!(messages.len(), 4, "{messages:?}");
    assert!(messages
        .iter()
        .all(|m| m.contains("http.StatusVariantAlsoNegotiates")));
}

#[test]
fn st1013_allows_whitelist_and_constants() {
    let dir = support::testdata("st1013");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1013/ok",
        &dir.join("ok.go"),
        &[("net/http", &dir.join("stub/net/http/http.go"))],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1013::analyzer(), &pkg).is_empty());
}

#[test]
fn st1018_flags_invisible_and_control_chars() {
    let dir = support::testdata("st1018");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/st1018");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1018::analyzer(), &pkg);
    assert!(messages.len() >= 5, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("control character U+0007")));
    assert!(messages.iter().any(|m| m.contains("control characters")));
    assert!(messages.iter().any(|m| m.contains("format character U+200B")));
    assert!(messages
        .iter()
        .any(|m| m.contains("format and control characters")));
}

#[test]
fn st1018_allows_escapes_and_emoji() {
    let dir = support::testdata("st1018");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/st1018/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1018::analyzer(), &pkg).is_empty());
}

#[test]
fn st1020_flags_badly_formed_exported_func_docs() {
    let dir = support::testdata("st1020");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/st1020");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1020::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(messages
        .iter()
        .any(|m| m.contains("exported function Bar")));
    assert!(messages.iter().any(|m| m.contains("exported method Bar")));
    assert!(messages.iter().any(|m| m.contains("exported function F3")));
    assert!(messages.iter().any(|m| m.contains("exported function F6")));
}

#[test]
fn st1020_allows_well_formed_exported_func_docs() {
    let dir = support::testdata("st1020");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/st1020/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1020::analyzer(), &pkg).is_empty());
}

#[test]
fn st1021_flags_badly_formed_exported_type_docs() {
    let dir = support::testdata("st1021");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/st1021");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1021::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("exported type T2")));
    assert!(messages.iter().any(|m| m.contains("exported type T4")));
    assert!(messages.iter().any(|m| m.contains("exported type T14")));
}

#[test]
fn st1021_allows_well_formed_exported_type_docs() {
    let dir = support::testdata("st1021");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/st1021/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1021::analyzer(), &pkg).is_empty());
}

#[test]
fn st1003_flags_poor_identifiers() {
    let dir = support::testdata("st1003");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/st1003");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1003::analyzer(), &pkg);
    assert!(messages.len() >= 5, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("abc_def")));
    assert!(messages.iter().any(|m| m.contains("ALL_CAPS")));
    assert!(messages.iter().any(|m| m.contains("fn_1")));
    assert!(messages.iter().any(|m| m.contains("fnId") && m.contains("fnID")));
    assert!(messages.iter().any(|m| m.contains("a_b")));
    assert!(messages.iter().any(|m| m.contains("e_f")));
    assert!(messages.iter().any(|m| m.contains("bad_field")));
}

#[test]
fn st1003_allows_good_identifiers() {
    let dir = support::testdata("st1003");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/st1003/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1003::analyzer(), &pkg).is_empty());
}

#[test]
fn st1003_flags_package_name_underscore() {
    let dir = support::testdata("st1003");
    let pkg = support::typecheck_file(&dir, "pkg_underscore.go", "example.com/staticcheck/st1003/u");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1003::analyzer(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("underscores in package names")),
        "{messages:?}"
    );
}

#[test]
fn st1003_flags_package_name_mixed_caps() {
    let dir = support::testdata("st1003");
    let pkg = support::typecheck_file(&dir, "pkg_mixed.go", "example.com/staticcheck/st1003/m");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1003::analyzer(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("MixedCaps")),
        "{messages:?}"
    );
}

#[test]
fn st1022_flags_badly_formed_exported_var_docs() {
    let dir = support::testdata("st1022");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/st1022");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1022::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("exported var B")));
    assert!(messages.iter().any(|m| m.contains("exported const D")));
    assert!(messages.iter().any(|m| m.contains("exported var I")));
}

#[test]
fn st1022_allows_well_formed_exported_var_docs() {
    let dir = support::testdata("st1022");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/st1022/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1022::analyzer(), &pkg).is_empty());
}

#[test]
fn st1023_flags_redundant_types() {
    let dir = support::testdata("st1023");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/st1023");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1023::analyzer(), &pkg);
    assert!(messages.len() >= 4, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("omit type int") && m.contains("inferred")));
    assert!(messages.iter().any(|m| m.contains("omit type bool")));
    assert!(messages.iter().any(|m| m.contains("omit type string")));
    assert!(messages.iter().any(|m| m.contains("omit type MyInt")));
}

/// The two axes of `sharedcheck.RedundantTypeInDeclarationChecker`: what the
/// right-hand side's type is when re-checked on its own, and — only when that
/// is untyped — which AST shapes survive `flagHelpfulTypes = false`. Counted by
/// declared type; `compat/golden/cases/staticcheck-st` pins the exact lines.
#[test]
fn st1023_isolates_the_right_hand_side() {
    let dir = support::testdata("st1023");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1023/isolated",
        &dir.join("isolated.go"),
        &[
            ("math", &dir.join("stub/math/math.go")),
            ("time", &dir.join("stub/time/time.go")),
        ],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1023::analyzer(), &pkg);
    assert_eq!(messages.len(), 17, "{messages:?}");
    let count = |t: &str| {
        messages
            .iter()
            .filter(|m| m.contains(&format!("omit type {t} from")))
            .count()
    };
    // A typed right-hand side is flagged whatever its shape: `5 * time.Second`
    // and `<-ch` are as redundant as a bare identifier.
    assert_eq!(count("time.Duration"), 2, "{messages:?}");
    // `var v int32 = 'a'` keeps its type — the default type of an untyped rune
    // is the alias `rune`, not `int32` — so the only two int32s here are the
    // typed-constant ones.
    assert_eq!(count("int32"), 2, "{messages:?}");
    assert_eq!(count("bool"), 2, "{messages:?}");
    assert_eq!(count("int"), 6, "{messages:?}");
}

#[test]
fn qf1011_isolates_the_right_hand_side() {
    let dir = support::testdata("qf1011");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/qf1011/isolated",
        &dir.join("isolated.go"),
        &[
            ("math", &dir.join("stub/math/math.go")),
            ("time", &dir.join("stub/time/time.go")),
        ],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1011::analyzer(), &pkg);
    assert_eq!(messages.len(), 27, "{messages:?}");
    let count = |t: &str| {
        messages
            .iter()
            .filter(|m| m.contains(&format!("omit type {t} from")))
            .count()
    };
    // Helpful types are flagged too, so untyped expressions and named
    // constants count here — but only where the default type still matches.
    assert_eq!(count("rune"), 3, "{messages:?}");
    assert_eq!(count("int32"), 2, "{messages:?}");
    // `var n uint = 1 << uint(x)` stays: a shift takes its left operand's
    // type, so the right-hand side is an untyped int whatever the count is.
    assert_eq!(count("uint"), 0, "{messages:?}");
}

#[test]
fn st1023_allows_necessary_types() {
    let dir = support::testdata("st1023");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/st1023/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1023::analyzer(), &pkg).is_empty());
}

#[test]
fn st1005_flags_bad_error_strings() {
    let dir = support::testdata("st1005");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1005",
        &dir.join("bad.go"),
        &[("errors", &dir.join("stub/errors/errors.go"))],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1005::analyzer(), &pkg);
    assert!(
        messages.iter().any(|m| m.contains("should not be capitalized")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("should not end with punctuation")),
        "{messages:?}"
    );
}

#[test]
fn st1005_allows_ok_error_strings() {
    let dir = support::testdata("st1005");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1005/ok",
        &dir.join("ok.go"),
        &[("errors", &dir.join("stub/errors/errors.go"))],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1005::analyzer(), &pkg).is_empty());
}

#[test]
fn st1005_ignores_non_stdlib_errors_new() {
    let dir = support::testdata("st1005");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1005/localerrors",
        &dir.join("local_errors.go"),
        &[(
            "example.com/local/errors",
            &dir.join("stub/localerrors/errors.go"),
        )],
    );
    support::assert_well_typed(&pkg);
    assert!(
        support::run_analyzer(st1005::analyzer(), &pkg).is_empty(),
        "non-stdlib errors.New must not trigger ST1005"
    );
}

#[test]
fn st1008_flags_error_not_last() {
    let dir = support::testdata("st1008");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/st1008");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1008::analyzer(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages
        .iter()
        .all(|m| m.contains("error should be returned as the last argument")));
}

#[test]
fn st1008_allows_error_last_and_comma_ok() {
    let dir = support::testdata("st1008");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/st1008/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1008::analyzer(), &pkg).is_empty());
}

#[test]
fn st1016_flags_inconsistent_receiver_names() {
    let dir = support::testdata("st1016");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/st1016");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(st1016::analyzer(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages
        .iter()
        .all(|m| m.contains("same receiver name")));
}

#[test]
fn st1016_allows_consistent_receivers() {
    let dir = support::testdata("st1016");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/st1016/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(st1016::analyzer(), &pkg).is_empty());
}

#[test]
fn st1001_dot_import_whitelist_allows_listed_packages() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_staticcheck::StylecheckOptions;

    let dir = support::testdata("st1001");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1001/whitelist",
        &dir.join("bad.go"),
        &[("fmt", &fmt_stub)],
    );
    support::assert_well_typed(&pkg);

    let mut bag = SettingsBag::new();
    bag.insert(
        "staticcheck",
        StylecheckOptions {
            dot_import_whitelist: Some(vec!["fmt".into()]),
            ..StylecheckOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        st1001::analyzer(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "dot-import-whitelist: fmt should allow import . \"fmt\": {messages:?}"
    );
}

#[test]
fn st1003_custom_initialisms_skip_id() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_staticcheck::StylecheckOptions;

    let dir = support::testdata("st1003");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/st1003/init");
    support::assert_well_typed(&pkg);

    let default_msgs = support::run_analyzer(st1003::analyzer(), &pkg);
    assert!(
        default_msgs
            .iter()
            .any(|m| m.contains("fnId") && m.contains("fnID")),
        "{default_msgs:?}"
    );

    let mut bag = SettingsBag::new();
    bag.insert(
        "staticcheck",
        StylecheckOptions {
            // Without ID, fnId should not be rewritten.
            initialisms: Some(vec!["HTTP".into(), "API".into()]),
            ..StylecheckOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        st1003::analyzer(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("fnId") && m.contains("fnID")),
        "custom initialisms without ID should not flag fnId: {messages:?}"
    );
    // Underscore names are still flagged.
    assert!(
        messages.iter().any(|m| m.contains("abc_def")),
        "{messages:?}"
    );
}

#[test]
fn st1013_custom_http_status_whitelist() {
    use std::sync::Arc;

    use guff_analysis::SettingsBag;
    use guff_runner::RunnerOptions;
    use guff_staticcheck::StylecheckOptions;

    let dir = support::testdata("st1013");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/st1013/whitelist",
        &dir.join("bad.go"),
        &[("net/http", &dir.join("stub/net/http/http.go"))],
    );
    support::assert_well_typed(&pkg);

    let mut bag = SettingsBag::new();
    bag.insert(
        "staticcheck",
        StylecheckOptions {
            http_status_code_whitelist: Some(vec![
                "200".into(),
                "400".into(),
                "404".into(),
                "500".into(),
                "506".into(),
            ]),
            ..StylecheckOptions::default()
        },
    );
    let messages = support::run_analyzer_with_settings(
        st1013::analyzer(),
        &pkg,
        &RunnerOptions {
            settings: Arc::new(bag),
            ..RunnerOptions::default()
        },
    );
    assert!(
        messages.is_empty(),
        "http-status-code-whitelist including 506 should silence ST1013: {messages:?}"
    );
}

#[test]
fn qf1009_flags_time_equality() {
    let dir = support::testdata("qf1009");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/qf1009",
        &dir.join("bad.go"),
        &[("time", &dir.join("stub/time/time.go"))],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1009::analyzer(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("time.Time.Equal")));
}

#[test]
fn qf1009_allows_equal_method() {
    let dir = support::testdata("qf1009");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/qf1009/ok",
        &dir.join("ok.go"),
        &[("time", &dir.join("stub/time/time.go"))],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(qf1009::analyzer(), &pkg).is_empty());
}

#[test]
fn qf1005_flags_expandable_pow() {
    let dir = support::testdata("qf1005");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/qf1005",
        &dir.join("bad.go"),
        &[("math", &dir.join("stub/math/math.go"))],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1005::analyzer(), &pkg);
    assert_eq!(messages.len(), 6, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("math.Pow")));
}

#[test]
fn qf1005_allows_non_expandable_pow() {
    let dir = support::testdata("qf1005");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/qf1005/ok",
        &dir.join("ok.go"),
        &[("math", &dir.join("stub/math/math.go"))],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(qf1005::analyzer(), &pkg).is_empty());
}

#[test]
fn qf1007_flags_conditional_assignment() {
    let dir = support::testdata("qf1007");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/qf1007");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1007::analyzer(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages
        .iter()
        .all(|m| m.contains("merge conditional assignment")));
}

#[test]
fn qf1007_allows_non_mergeable() {
    let dir = support::testdata("qf1007");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/qf1007/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(qf1007::analyzer(), &pkg).is_empty());
}

#[test]
fn qf1004_flags_replace_split_with_minus_one() {
    let dir = support::testdata("qf1004");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/qf1004",
        &dir.join("bad.go"),
        &[
            ("strings", &dir.join("stub/strings/strings.go")),
            ("bytes", &dir.join("stub/bytes/bytes.go")),
        ],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1004::analyzer(), &pkg);
    assert_eq!(messages.len(), 6, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("ReplaceAll")));
    assert!(messages.iter().any(|m| m.contains("Split")));
}

#[test]
fn qf1004_allows_replace_all_and_nonzero_n() {
    let dir = support::testdata("qf1004");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/qf1004/ok",
        &dir.join("ok.go"),
        &[
            ("strings", &dir.join("stub/strings/strings.go")),
            ("bytes", &dir.join("stub/bytes/bytes.go")),
        ],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(qf1004::analyzer(), &pkg).is_empty());
}

#[test]
fn qf1004_uses_renamed_import_in_suggested_fix() {
    let dir = support::testdata("qf1004");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/qf1004/renamed",
        &dir.join("renamed.go"),
        &[
            ("strings", &dir.join("stub/strings/strings.go")),
            ("bytes", &dir.join("stub/bytes/bytes.go")),
        ],
    );
    support::assert_well_typed(&pkg);
    // The message names the canonical function; only the suggested fix uses
    // the file's own alias (upstream behaviour, checked against golangci-lint).
    let diags = support::run_analyzer_diagnostics(qf1004::analyzer(), &pkg);
    assert_eq!(diags.len(), 2, "{diags:?}");
    assert!(diags.iter().any(|d| d.message.contains("strings.ReplaceAll")));
    assert!(diags.iter().any(|d| d.message.contains("bytes.ReplaceAll")));
    let fixes: Vec<String> = diags
        .iter()
        .flat_map(|d| d.suggested_fixes.iter())
        .flat_map(|f| f.text_edits.iter())
        .map(|e| e.new_text.clone())
        .collect();
    assert!(fixes.iter().any(|t| t.contains("s.ReplaceAll")), "{fixes:?}");
    assert!(fixes.iter().any(|t| t.contains("b.ReplaceAll")), "{fixes:?}");
}

#[test]
fn qf1006_flags_lift_if_break() {
    let dir = support::testdata("qf1006");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/qf1006");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1006::analyzer(), &pkg);
    assert_eq!(messages.len(), 5, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("lift into loop condition")));
}

#[test]
fn qf1006_allows_non_liftable() {
    let dir = support::testdata("qf1006");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/qf1006/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(qf1006::analyzer(), &pkg).is_empty());
}

#[test]
fn qf1010_flags_byte_slice_printing() {
    let dir = support::testdata("qf1010");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/qf1010",
        &dir.join("bad.go"),
        &[("fmt", &dir.join("stub/fmt/fmt.go"))],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1010::analyzer(), &pkg);
    assert_eq!(messages.len(), 6, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("convert argument to string")));
}

#[test]
fn qf1010_allows_non_byte_slice() {
    let dir = support::testdata("qf1010");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/qf1010/ok",
        &dir.join("ok.go"),
        &[("fmt", &dir.join("stub/fmt/fmt.go"))],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(qf1010::analyzer(), &pkg).is_empty());
}

#[test]
fn qf1011_flags_redundant_types() {
    let dir = support::testdata("qf1011");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/qf1011");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1011::analyzer(), &pkg);
    assert!(messages.len() >= 5, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("could omit type int")));
    assert!(messages.iter().any(|m| m.contains("could omit type bool")));
    assert!(messages.iter().any(|m| m.contains("could omit type string")));
    assert!(messages.iter().any(|m| m.contains("could omit type MyInt")));
}

#[test]
fn qf1011_allows_necessary_types() {
    let dir = support::testdata("qf1011");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/qf1011/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(qf1011::analyzer(), &pkg).is_empty());
}

#[test]
fn qf1012_flags_write_sprintf() {
    let dir = support::testdata("qf1012");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/qf1012",
        &dir.join("bad.go"),
        &[
            ("fmt", &dir.join("stub/fmt/fmt.go")),
            ("io", &dir.join("stub/io/io.go")),
        ],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1012::analyzer(), &pkg);
    assert_eq!(messages.len(), 6, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("Fprint")));
    assert!(messages.iter().any(|m| m.contains("Fprintf")));
    assert!(messages.iter().any(|m| m.contains("Fprintln")));
}

#[test]
fn qf1012_allows_non_writer() {
    let dir = support::testdata("qf1012");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/qf1012/ok",
        &dir.join("ok.go"),
        &[
            ("fmt", &dir.join("stub/fmt/fmt.go")),
            ("io", &dir.join("stub/io/io.go")),
        ],
    );
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(qf1012::analyzer(), &pkg).is_empty());
}

#[test]
fn qf1001_flags_demorgan() {
    let dir = support::testdata("qf1001");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/qf1001");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1001::analyzer(), &pkg);
    assert_eq!(messages.len(), 2);
    assert!(messages[0].contains("De Morgan"));
}

#[test]
fn qf1001_skips_floats_and_ok() {
    let dir = support::testdata("qf1001");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/qf1001/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(qf1001::analyzer(), &pkg).is_empty());
}

#[test]
fn qf1002_flags_tagless_switch() {
    let dir = support::testdata("qf1002");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/qf1002");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1002::analyzer(), &pkg);
    assert!(messages.len() >= 2);
    assert!(messages.iter().any(|m| m.contains("tagged switch on x")));
    assert!(messages.iter().any(|m| m.contains("tagged switch on a")));
}

#[test]
fn qf1002_allows_tagged_and_mixed() {
    let dir = support::testdata("qf1002");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/qf1002/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(qf1002::analyzer(), &pkg).is_empty());
}

#[test]
fn qf1003_flags_if_else_chain() {
    let dir = support::testdata("qf1003");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/qf1003");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1003::analyzer(), &pkg);
    assert!(messages.len() >= 3);
    assert!(messages.iter().any(|m| m.contains("tagged switch on x")));
    assert!(messages.iter().any(|m| m.contains("tagged switch on a")));
}

#[test]
fn qf1003_allows_non_convertible() {
    let dir = support::testdata("qf1003");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/qf1003/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(qf1003::analyzer(), &pkg).is_empty());
}

#[test]
fn qf1008_flags_embedded_selector() {
    let dir = support::testdata("qf1008");
    let pkg = support::typecheck_file(&dir, "bad.go", "example.com/staticcheck/qf1008");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1008::analyzer(), &pkg);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("BasicInner"));
}

#[test]
fn qf1008_flags_interrupted_call_chain() {
    let dir = support::testdata("qf1008");
    let pkg = support::typecheck_file(&dir, "call.go", "example.com/staticcheck/qf1008/call");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1008::analyzer(), &pkg);
    // `call.FunctionCallInner.F8().FunctionCallContinuedInner.F9` splits into two
    // segments and upstream flags both (golangci-lint's default
    // `issues.uniq-by-line` is what hides the second one in its output).
    // Leaf call `o.MethodInner.M()` is flagged too.
    assert!(
        messages.iter().any(|m| m.contains("FunctionCallInner")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("FunctionCallContinuedInner")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("MethodInner")),
        "{messages:?}"
    );
}

#[test]
fn qf1008_skips_selectors_enclosed_by_another_selector() {
    let dir = support::testdata("qf1008");
    let pkg = support::typecheck_file(&dir, "nested.go", "example.com/staticcheck/qf1008/nested");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(qf1008::analyzer(), &pkg);
    // golangci-lint 2.12 with `uniq-by-line: false` reports exactly two here:
    // the outer `sink(…).NestedInner.F1` and the one in `fnNotNested`. The two
    // enclosed by a `harness{…}.Run` selector are skipped.
    assert_eq!(messages.len(), 2, "{messages:?}");
}

#[test]
fn qf1008_allows_minimal_and_non_embedded() {
    let dir = support::testdata("qf1008");
    let pkg = support::typecheck_file(&dir, "ok.go", "example.com/staticcheck/qf1008/ok");
    support::assert_well_typed(&pkg);
    assert!(support::run_analyzer(qf1008::analyzer(), &pkg).is_empty());
}

#[test]
fn sa4010_allows_shadowed_pairs_returned() {
    let pkg = typecheck_rule("sa4010", "shadow_return_ok.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa4010::analyzer(), &pkg);
    assert!(
        messages.is_empty(),
        "SA4010 FP on returned append with inner pairs shadow: {messages:?}"
    );
}

#[test]
fn sa4010_allows_converter_style_returned_append() {
    // Grafana converter.readLabelsOrExemplars: outer pairs appended in a
    // labeled for/switch, with an inner `pairs :=` shadow in another case,
    // then `return …, pairs, nil`. Must not flag SA4010.
    let pkg = typecheck_rule("sa4010", "converter_fp.go");
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa4010::analyzer(), &pkg);
    assert!(
        messages.is_empty(),
        "SA4010 FP on converter-style returned append: {messages:?}"
    );
}
