mod support;

use guff_staticcheck::{sa4000, sa4001, sa4003, sa4004, sa4005, sa4006, sa4008, sa4009, sa4010, sa4011, sa4012, sa4013, sa4014, sa4015, sa4016, sa4017, sa4018, sa4019, sa4020, sa4021, sa4022, sa4023, sa4024, sa4025, sa4026, sa4027, sa4028, sa4029, sa4030, sa4031, sa4032, sa1000, sa1001, sa1002, sa1003, sa1004, sa1005, sa1006, sa1007, sa1008, sa1010, sa1011, sa1012, sa1013, sa1014, sa1015, sa1016, sa1017, sa1018, sa1019, sa1020, sa1021, sa1023, sa1024, sa1025, sa1026, sa1027, sa1028, sa1029, sa1030, sa1031, sa1032, sa2000, sa2001, sa2002, sa2003, sa3000, sa3001, sa5000, sa5001, sa5002, sa5003, sa5004, sa5005, sa5007, sa5008, sa5009, sa5010, sa5011, sa5012, sa6000, sa6001, sa6002, sa6003, sa6005, sa6006, sa9001, sa9002, sa9003, sa9004, sa9005, sa9006, sa9007, sa9008, sa9009, sa9010, s1000, s1001, s1003, s1004, s1005, s1006, s1007, s1008, s1009, s1010, s1011, s1012, s1016, s1017, s1018, s1019, s1020, s1021, s1023, s1024, s1025, s1028, s1029, s1030, s1031, s1032, s1033, s1034, s1035, s1036, s1037, s1038, s1039, s1040, st1001, st1006, st1012, st1015};
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

    let messages = support::run_analyzer(sa1000::analyzer(), &pkg);
    assert!(messages.len() >= 3, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("error parsing regexp")));
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

    let messages = support::run_analyzer(sa1002::analyzer(), &pkg);
    assert!(messages.len() >= 2, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("parsing time")));
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
    let messages = support::run_analyzer(sa1007::analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("is not a valid URL")));
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
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1014",
        &dir.join("bad.go"),
        &[("encoding/json", &json_stub)],
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
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1014/ok",
        &dir.join("ok.go"),
        &[("encoding/json", &json_stub)],
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
#[ignore = "SC-D08: guff string literals for \\xNN (NN>=0x80) differ from Go byte strings"]
fn sa1011_flags_invalid_utf8_cutsets() {
    let dir = support::testdata("sa1011");
    let strings_stub = dir.join("stub/strings/strings.go");
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1011",
        &dir.join("bad.go"),
        &[("strings", &strings_stub)],
    );
    support::assert_well_typed(&pkg);

    let messages = support::run_analyzer(sa1011::analyzer(), &pkg);
    assert!(messages.len() >= 2, "{messages:?}");
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
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1003",
        &dir.join("bad.go"),
        &[("encoding/binary", &binary_stub)],
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
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1003/ok",
        &dir.join("ok.go"),
        &[("encoding/binary", &binary_stub)],
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
    assert!(messages.iter().any(|m| m.contains("copy()")));
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
    assert!(messages.iter().any(|m| m.contains("append(lhs, x...)")));
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
    assert!(messages.iter().any(|m| m.contains("make(T)")));
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
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("time.Sleep")));
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
    let pkg = support::typecheck_with_deps(
        "example.com/staticcheck/sa1001",
        &dir.join("bad.go"),
        &[("text/template", &tmpl_stub)],
    );
    support::assert_well_typed(&pkg);
    let messages = support::run_analyzer(sa1001::analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("unexpected"));
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
sa_check!(sa1023, sa1023_flags_writer_buffer_modified, sa1023_allows_readonly_write, "must not modify the provided buffer");
sa_check!(sa1025, sa1025_flags_timer_reset_return, sa1025_allows_timer_reset_without_drain, "Reset's return value");

sa_check_bad_ok!(sa4000, sa4000_flags_bad_cases, sa4000_allows_ok_cases);
sa_check_bad_ok!(sa4001, sa4001_flags_bad_cases, sa4001_allows_ok_cases);
sa_check_bad_ok!(sa4003, sa4003_flags_bad_cases, sa4003_allows_ok_cases);
sa_check_bad_ok!(sa4004, sa4004_flags_bad_cases, sa4004_allows_ok_cases);
sa_check_bad_ok!(sa4005, sa4005_flags_bad_cases, sa4005_allows_ok_cases);
sa_check_bad_ok!(sa4006, sa4006_flags_bad_cases, sa4006_allows_ok_cases);
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
sa_check!(sa9010, sa9010_flags_uncalled_defer_return, sa9010_allows_called_defer_return, "deferred return function");

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
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("Printf"), "{messages:?}");
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
