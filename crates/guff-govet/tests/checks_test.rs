mod support;

use guff_govet::{
    appends_analyzer, assign_analyzer, atomic_analyzer, bools_analyzer, buildtag_analyzer,
    cgocall_analyzer, composites_analyzer, copylocks_analyzer, defers_analyzer,
    directive_analyzer, errorsas_analyzer, framepointer_analyzer, httpresponse_analyzer,
    hostport_analyzer, ifaceassert_analyzer, inline_analyzer, loopclosure_analyzer, lostcancel_analyzer,
    nilfunc_analyzer, printf_analyzer, shift_analyzer, sigchanyzer_analyzer, slog_analyzer,
    stdmethods_analyzer, stringintconv_analyzer, structtag_analyzer, tests_analyzer,
    timeformat_analyzer, unmarshal_analyzer, unreachable_analyzer, unsafeptr_analyzer,
    unusedresult_analyzer, waitgroup_analyzer,
};
use guff_types::Config;

#[test]
fn copylocks_flags_value_param() {
    let dir = support::testdata("copylocks");
    let stub = dir.join("stub/sync/mutex.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/copylocks",
        &dir.join("bad.go"),
        &[("sync", &stub)],
    );
    let messages = support::run_analyzer(copylocks_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("lock")));
    assert!(
        messages.iter().any(|m| m.contains("return copies lock")),
        "{messages:?}"
    );
}

#[test]
fn copylocks_allows_pointer_param() {
    let dir = support::testdata("copylocks");
    let stub = dir.join("stub/sync/mutex.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/copylocks/ok",
        &dir.join("ok.go"),
        &[("sync", &stub)],
    );
    assert!(support::run_analyzer(copylocks_analyzer(), &pkg).is_empty());
}

#[test]
fn printf_flags_unknown_verb() {
    let dir = support::testdata("printf");
    let stub = dir.join("stub/fmt/print.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/printf",
        &dir.join("bad.go"),
        &[("fmt", &stub)],
    );
    let messages = support::run_analyzer(printf_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("unknown verb"));
}

#[test]
fn printf_allows_valid_format() {
    let dir = support::testdata("printf");
    let stub = dir.join("stub/fmt/print.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/printf/ok",
        &dir.join("ok.go"),
        &[("fmt", &stub)],
    );
    assert!(support::run_analyzer(printf_analyzer(), &pkg).is_empty());
}

#[test]
fn printf_flags_wrong_arg_type() {
    let dir = support::testdata("printf");
    let stub = dir.join("stub/fmt/print.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/printf/wrongtype",
        &dir.join("wrongtype.go"),
        &[("fmt", &stub)],
    );
    let messages = support::run_analyzer(printf_analyzer(), &pkg);
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("wrong type")));
    assert!(messages.iter().any(|m| m.contains("%d") && m.contains("string")));
    assert!(messages.iter().any(|m| m.contains("%s") && m.contains("int")));
    assert!(messages.iter().any(|m| m.contains("%t") && m.contains("int")));
}

#[test]
fn printf_flags_arg_count() {
    let dir = support::testdata("printf");
    let stub = dir.join("stub/fmt/print.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/printf/argcount",
        &dir.join("argcount.go"),
        &[("fmt", &stub)],
    );
    let messages = support::run_analyzer(printf_analyzer(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("reads arg #2, but call has 1 arg")));
    assert!(messages.iter().any(|m| m.contains("needs 1 arg but has 2 args")));
}

#[test]
fn printf_allows_stringer_and_composites() {
    let dir = support::testdata("printf");
    let stub = dir.join("stub/fmt/print.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/printf/ok2",
        &dir.join("ok2.go"),
        &[("fmt", &stub)],
    );
    assert!(support::run_analyzer(printf_analyzer(), &pkg).is_empty(), "{:?}", support::run_analyzer(printf_analyzer(), &pkg));
}

#[test]
fn assign_flags_self_assignment() {
    let dir = support::testdata("assign");
    let pkg = support::typecheck_pkg("example.com/govet/assign", &dir.join("bad.go"));
    let messages = support::run_analyzer(assign_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("self-assignment"));
}

#[test]
fn assign_allows_distinct_assignment() {
    let dir = support::testdata("assign");
    let pkg = support::typecheck_pkg("example.com/govet/assign/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(assign_analyzer(), &pkg).is_empty());
}

#[test]
fn shift_flags_oversized_shift() {
    let dir = support::testdata("shift");
    let pkg = support::typecheck_pkg("example.com/govet/shift", &dir.join("bad.go"));
    let messages = support::run_analyzer(shift_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("too small for shift"));
}

#[test]
fn shift_allows_small_shift() {
    let dir = support::testdata("shift");
    let pkg = support::typecheck_pkg("example.com/govet/shift/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(shift_analyzer(), &pkg).is_empty());
}

#[test]
fn stringintconv_flags_int_to_string() {
    let dir = support::testdata("stringintconv");
    let pkg = support::typecheck_pkg("example.com/govet/stringintconv", &dir.join("bad.go"));
    let messages = support::run_analyzer(stringintconv_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("one rune"));
}

#[test]
fn stringintconv_allows_rune_conversion() {
    let dir = support::testdata("stringintconv");
    let pkg = support::typecheck_pkg("example.com/govet/stringintconv/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(stringintconv_analyzer(), &pkg).is_empty());
}

#[test]
fn inline_flags_reflect_ptr() {
    let dir = support::testdata("inline");
    let stub = dir.join("stub/reflect/reflect.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/inline",
        &dir.join("bad.go"),
        &[("reflect", &stub)],
    );
    let messages = support::run_analyzer(inline_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(messages[0], "Constant reflect.Ptr should be inlined");
}

#[test]
fn inline_allows_reflect_pointer() {
    let dir = support::testdata("inline");
    let stub = dir.join("stub/reflect/reflect.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/inline/ok",
        &dir.join("ok.go"),
        &[("reflect", &stub)],
    );
    assert!(support::run_analyzer(inline_analyzer(), &pkg).is_empty());
}

#[test]
fn inline_flags_exp_maps_clone_type_param_gap() {
    let dir = support::testdata("inline_exp");
    let stub = dir.join("stub/golang.org/x/exp/maps/maps.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/inline_exp",
        &dir.join("bad.go"),
        &[("golang.org/x/exp/maps", &stub)],
    );
    let messages = support::run_analyzer(inline_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(
        messages[0],
        "cannot inline: type parameter inference is not yet supported"
    );
}

#[test]
fn inline_flags_local_go_fix_const() {
    let dir = support::testdata("inline_local");
    let pkg = support::typecheck_pkg("example.com/govet/inline_local", &dir.join("bad.go"));
    let messages = support::run_analyzer(inline_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(messages[0], "Constant Legacy should be inlined");
}

#[test]
fn inline_allows_preferred_const() {
    let dir = support::testdata("inline_local");
    let pkg = support::typecheck_pkg("example.com/govet/inline_local/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(inline_analyzer(), &pkg).is_empty());
}

#[test]
fn inline_flags_ioutil_go_version_mismatch() {
    let dir = support::testdata("inline_ioutil");
    let stub = dir.join("stub/io/ioutil/ioutil.go");
    let pkg = support::with_go_version(
        support::typecheck_with_deps(
            "example.com/govet/inline_ioutil",
            &dir.join("bad.go"),
            &[("io/ioutil", &stub)],
        ),
        "1.24.3",
    );
    let messages = support::run_analyzer(inline_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(
        messages[0].starts_with("cannot inline call to ioutil.TempDir (declared using go"),
        "{messages:?}"
    );
    assert!(
        messages[0].contains("into a file using go1.24.3"),
        "{messages:?}"
    );
}

#[test]
fn errorsas_flags_non_pointer_target() {
    let dir = support::testdata("errorsas");
    let pkg = support::typecheck_pkg("example.com/govet/errorsas", &dir.join("bad.go"));
    let messages = support::run_analyzer(errorsas_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("errors.As"));
}

#[test]
fn errorsas_allows_pointer_target() {
    let dir = support::testdata("errorsas");
    let pkg = support::typecheck_pkg("example.com/govet/errorsas/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(errorsas_analyzer(), &pkg).is_empty());
}

#[test]
fn errorsas_allows_concrete_error_pointer() {
    let dir = support::testdata("errorsas");
    let pkg = support::typecheck_pkg(
        "example.com/govet/errorsas/concrete_ok",
        &dir.join("concrete_ok.go"),
    );
    assert!(support::run_analyzer(errorsas_analyzer(), &pkg).is_empty());
}

#[test]
fn errorsas_flags_non_pointer_concrete_error() {
    let dir = support::testdata("errorsas");
    let pkg = support::typecheck_pkg(
        "example.com/govet/errorsas/concrete_bad",
        &dir.join("concrete_bad.go"),
    );
    let messages = support::run_analyzer(errorsas_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages[0].contains("errors.As"));
}

#[test]
fn defers_flags_undelayed_since() {
    let dir = support::testdata("defers");
    let time_stub = dir.join("stub/time/time.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/defers",
        &dir.join("bad.go"),
        &[("time", &time_stub)],
    );
    let messages = support::run_analyzer(defers_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("time.Since")));
}

#[test]
fn defers_allows_guarded_since() {
    let dir = support::testdata("defers");
    let time_stub = dir.join("stub/time/time.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/defers/ok",
        &dir.join("ok.go"),
        &[("time", &time_stub)],
    );
    assert!(support::run_analyzer(defers_analyzer(), &pkg).is_empty());
}

#[test]
fn atomic_flags_direct_assignment() {
    let dir = support::testdata("atomic");
    let atomic_stub = dir.join("stub/sync/atomic/atomic.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/atomic",
        &dir.join("bad.go"),
        &[("sync/atomic", &atomic_stub)],
    );
    let messages = support::run_analyzer(atomic_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("atomic value"));
}

#[test]
fn atomic_allows_discarded_result() {
    let dir = support::testdata("atomic");
    let atomic_stub = dir.join("stub/sync/atomic/atomic.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/atomic/ok",
        &dir.join("ok.go"),
        &[("sync/atomic", &atomic_stub)],
    );
    assert!(support::run_analyzer(atomic_analyzer(), &pkg).is_empty());
}

#[test]
fn unusedresult_flags_fmt_errorf() {
    let dir = support::testdata("unusedresult");
    let pkg = support::typecheck_pkg("example.com/govet/unusedresult", &dir.join("bad.go"));
    let messages = support::run_analyzer(unusedresult_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("errors.New"));
}

#[test]
fn bools_flags_redundant_or() {
    let dir = support::testdata("bools");
    let pkg = support::typecheck_pkg("example.com/govet/bools", &dir.join("bad.go"));
    let messages = support::run_analyzer(bools_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("redundant"));
}

#[test]
fn bools_distinct_selectors_not_redundant() {
    // `c.a == 0 && c.b == 0` compares different fields and must not be flagged;
    // `c.a == 0 && c.a == 0` (same field) still must be. Regression for the
    // selector-operand `_` collapse that produced spurious "redundant and".
    let dir = support::testdata("bools_sel");
    let pkg = support::typecheck_pkg("example.com/govet/boolssel", &dir.join("main.go"));
    let messages = support::run_analyzer(bools_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "expected only the identical-field case: {messages:?}");
    assert!(messages[0].contains("redundant"), "{messages:?}");
}

#[test]
fn structtag_flags_unexported_json() {
    let dir = support::testdata("structtag");
    let pkg = support::typecheck_pkg("example.com/govet/structtag", &dir.join("bad.go"));
    let messages = support::run_analyzer(structtag_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("not exported"));
}

#[test]
fn structtag_allows_embedded_json_tag() {
    let dir = support::testdata("structtag");
    let pkg = support::typecheck_pkg("example.com/govet/structtag/ok", &dir.join("ok.go"));
    let messages = support::run_analyzer(structtag_analyzer(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn composites_flags_unkeyed_imported_struct() {
    let dir = support::testdata("composites");
    let stub = dir.join("stub/other/config.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/composites",
        &dir.join("bad.go"),
        &[("example.com/govet/composites/other", &stub)],
    );
    let messages = support::run_analyzer(composites_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("unkeyed fields"));
}

#[test]
fn composites_allows_keyed_imported_struct() {
    let dir = support::testdata("composites");
    let stub = dir.join("stub/other/config.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/composites/ok",
        &dir.join("ok.go"),
        &[("example.com/govet/composites/other", &stub)],
    );
    assert!(support::run_analyzer(composites_analyzer(), &pkg).is_empty());
}

#[test]
fn hostport_flags_sprintf_address_formats() {
    let dir = support::testdata("hostport");
    let net_stub = dir.join("stub/net/net.go");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/hostport",
        &dir.join("bad.go"),
        &[("net", &net_stub), ("fmt", &fmt_stub)],
    );
    let messages = support::run_analyzer(hostport_analyzer(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages
        .iter()
        .any(|m| m.contains(r#"address format "%s:%d" does not work with IPv6"#)));
    // The variable form names the dial's line; the direct form does not.
    assert!(messages.iter().any(|m| m.contains("passed to net.Dial at L")));
}

#[test]
fn hostport_allows_joinhostport_and_undialed_formats() {
    let dir = support::testdata("hostport");
    let net_stub = dir.join("stub/net/net.go");
    let fmt_stub = dir.join("stub/fmt/fmt.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/hostport/ok",
        &dir.join("ok.go"),
        &[("net", &net_stub), ("fmt", &fmt_stub)],
    );
    assert!(support::run_analyzer(hostport_analyzer(), &pkg).is_empty());
}

#[test]
fn appends_flags_append_with_no_values() {
    let dir = support::testdata("appends");
    let pkg = support::typecheck_pkg("example.com/govet/appends", &dir.join("bad.go"));
    let messages = support::run_analyzer(appends_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("append with no values"));
}

#[test]
fn appends_allows_values_spread_and_a_shadowed_builtin() {
    let dir = support::testdata("appends");
    let pkg = support::typecheck_pkg("example.com/govet/appends/ok", &dir.join("ok.go"));
    assert!(
        support::run_analyzer(appends_analyzer(), &pkg).is_empty(),
        "a local named `append` is not the builtin"
    );
}

#[test]
fn waitgroup_flags_add_inside_the_goroutine() {
    let dir = support::testdata("waitgroup");
    let stub = dir.join("stub/sync/sync.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/waitgroup",
        &dir.join("bad.go"),
        &[("sync", &stub)],
    );
    let messages = support::run_analyzer(waitgroup_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("WaitGroup.Add called from inside new goroutine"));
}

#[test]
fn waitgroup_wants_add_as_the_goroutines_first_statement() {
    let dir = support::testdata("waitgroup");
    let stub = dir.join("stub/sync/sync.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/waitgroup/ok",
        &dir.join("ok.go"),
        &[("sync", &stub)],
    );
    assert!(
        support::run_analyzer(waitgroup_analyzer(), &pkg).is_empty(),
        "upstream matches a fixed stack shape whose ExprStmt is Block.List[0]"
    );
}

#[test]
fn nilfunc_flags_func_nil_comparison() {
    let dir = support::testdata("nilfunc");
    let pkg = support::typecheck_pkg("example.com/govet/nilfunc", &dir.join("bad.go"));
    let messages = support::run_analyzer(nilfunc_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("comparison of function"));
}

#[test]
fn nilfunc_allows_pointer_nil_comparison() {
    let dir = support::testdata("nilfunc");
    let pkg = support::typecheck_pkg("example.com/govet/nilfunc/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(nilfunc_analyzer(), &pkg).is_empty());
}

#[test]
fn unreachable_flags_code_after_return() {
    let dir = support::testdata("unreachable");
    let pkg = support::typecheck_pkg("example.com/govet/unreachable", &dir.join("bad.go"));
    let messages = support::run_analyzer(unreachable_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("unreachable code"));
}

#[test]
fn unreachable_allows_conditional_return() {
    let dir = support::testdata("unreachable");
    let pkg = support::typecheck_pkg("example.com/govet/unreachable/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(unreachable_analyzer(), &pkg).is_empty());
}

#[test]
fn buildtag_flags_misplaced_directive() {
    let dir = support::testdata("buildtag");
    let pkg = support::typecheck_pkg("example.com/govet/buildtag", &dir.join("bad.go"));
    let messages = support::run_analyzer(buildtag_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("go:build")));
}

#[test]
fn buildtag_allows_header_directive() {
    let dir = support::testdata("buildtag");
    let pkg = support::typecheck_pkg("example.com/govet/buildtag/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(buildtag_analyzer(), &pkg).is_empty());
}

#[test]
fn directive_flags_debug_in_library() {
    let dir = support::testdata("directive");
    let pkg = support::typecheck_pkg("example.com/govet/directive", &dir.join("bad.go"));
    let messages = support::run_analyzer(directive_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("go:debug")));
}

#[test]
fn directive_allows_main_package() {
    let dir = support::testdata("directive");
    let pkg = support::typecheck_pkg("example.com/govet/directive/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(directive_analyzer(), &pkg).is_empty());
}

#[test]
fn cgocall_flags_chan_argument() {
    let dir = support::testdata("cgocall");
    let pkg = support::typecheck_with_config(
        "example.com/govet/cgocall",
        &dir.join("bad.go"),
        &[],
        Config {
            fake_import_c: true,
            ..Config::default()
        },
    );
    let messages = support::run_analyzer(cgocall_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("pointer")));
}

#[test]
fn cgocall_allows_scalar_argument() {
    let dir = support::testdata("cgocall");
    let pkg = support::typecheck_with_config(
        "example.com/govet/cgocall/ok",
        &dir.join("ok.go"),
        &[],
        Config {
            fake_import_c: true,
            ..Config::default()
        },
    );
    assert!(support::run_analyzer(cgocall_analyzer(), &pkg).is_empty());
}

#[test]
fn ifaceassert_flags_impossible_assertion() {
    let dir = support::testdata("ifaceassert");
    let pkg = support::typecheck_pkg("example.com/govet/ifaceassert", &dir.join("bad.go"));
    let messages = support::run_analyzer(ifaceassert_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("impossible type assertion")));
}

#[test]
fn ifaceassert_allows_compatible_interfaces() {
    let dir = support::testdata("ifaceassert");
    let pkg = support::typecheck_pkg("example.com/govet/ifaceassert/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(ifaceassert_analyzer(), &pkg).is_empty());
}

#[test]
fn loopclosure_flags_captured_loop_var() {
    let dir = support::testdata("loopclosure");
    // loopclosure only reports before Go 1.22 (per-iteration loop variables).
    // Pin the module version so the check does not depend on whatever Go
    // toolchain happens to be installed.
    let pkg = support::with_go_version(
        support::typecheck_pkg("example.com/govet/loopclosure", &dir.join("bad.go")),
        "1.21",
    );
    let messages = support::run_analyzer(loopclosure_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("loop variable")));
}

#[test]
fn loopclosure_allows_shadowed_loop_var() {
    let dir = support::testdata("loopclosure");
    let pkg = support::typecheck_pkg("example.com/govet/loopclosure/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(loopclosure_analyzer(), &pkg).is_empty());
}

#[test]
fn sigchanyzer_flags_unbuffered_notify() {
    let dir = support::testdata("sigchanyzer");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/sigchanyzer",
        &dir.join("bad.go"),
        &[
            ("os/signal", &dir.join("stub/os/signal/signal.go")),
            ("os", &dir.join("stub/os/os.go")),
        ],
    );
    let messages = support::run_analyzer(sigchanyzer_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("signal.Notify")));
}

/// `signal.Notify(make(chan os.Signal), ...)` is the one shape upstream
/// exempts (golang/go#45043); the condition used to be inverted, so this was
/// the only shape guff reported.
#[test]
fn sigchanyzer_allows_inline_make() {
    let dir = support::testdata("sigchanyzer");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/sigchanyzer/inlinemake",
        &dir.join("inline_make.go"),
        &[
            ("os/signal", &dir.join("stub/os/signal/signal.go")),
            ("os", &dir.join("stub/os/os.go")),
        ],
    );
    assert!(support::run_analyzer(sigchanyzer_analyzer(), &pkg).is_empty());
}

#[test]
fn sigchanyzer_allows_buffered_channel() {
    let dir = support::testdata("sigchanyzer");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/sigchanyzer/ok",
        &dir.join("ok.go"),
        &[
            ("os/signal", &dir.join("stub/os/signal/signal.go")),
            ("os", &dir.join("stub/os/os.go")),
        ],
    );
    assert!(support::run_analyzer(sigchanyzer_analyzer(), &pkg).is_empty());
}

#[test]
fn slog_flags_missing_value() {
    let dir = support::testdata("slog");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/slog",
        &dir.join("bad.go"),
        &[("log/slog", &dir.join("stub/log/slog/slog.go"))],
    );
    let messages = support::run_analyzer(slog_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("slog")));
}

#[test]
fn slog_allows_balanced_pairs() {
    let dir = support::testdata("slog");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/slog/ok",
        &dir.join("ok.go"),
        &[("log/slog", &dir.join("stub/log/slog/slog.go"))],
    );
    assert!(support::run_analyzer(slog_analyzer(), &pkg).is_empty());
}

#[test]
fn slog_allows_attr_helpers() {
    // slog.Any / slog.String return slog.Attr; must not be flagged as a missing
    // key/value (regression for is_type_named checking underlying()).
    let dir = support::testdata("slog");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/slog/attr_ok",
        &dir.join("attr_ok.go"),
        &[("log/slog", &dir.join("stub/log/slog/slog.go"))],
    );
    assert!(
        support::run_analyzer(slog_analyzer(), &pkg).is_empty(),
        "{:?}",
        support::run_analyzer(slog_analyzer(), &pkg)
    );
}

#[test]
fn stdmethods_flags_bad_unwrap() {
    let dir = support::testdata("stdmethods");
    let pkg = support::typecheck_pkg("example.com/govet/stdmethods", &dir.join("bad.go"));
    let messages = support::run_analyzer(stdmethods_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("Unwrap")));
    // Non-error receivers / interface Unwrap() T must not be flagged (x/tools parity).
    assert_eq!(
        messages.iter().filter(|m| m.contains("Unwrap")).count(),
        1,
        "expected only MyError.Unwrap FP, got {messages:?}"
    );
}

#[test]
fn stdmethods_allows_correct_unwrap() {
    let dir = support::testdata("stdmethods");
    let pkg = support::typecheck_pkg("example.com/govet/stdmethods/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(stdmethods_analyzer(), &pkg).is_empty());
}

#[test]
fn tests_flags_malformed_name() {
    let dir = support::testdata("tests");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/tests",
        &dir.join("bad_test.go"),
        &[("testing", &dir.join("stub/testing/testing.go"))],
    );
    let messages = support::run_analyzer(tests_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("malformed name")));
}

#[test]
fn tests_allows_valid_name() {
    let dir = support::testdata("tests");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/tests/ok",
        &dir.join("ok_test.go"),
        &[("testing", &dir.join("stub/testing/testing.go"))],
    );
    assert!(support::run_analyzer(tests_analyzer(), &pkg).is_empty());
}

#[test]
fn timeformat_flags_bad_layout() {
    let dir = support::testdata("timeformat");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/timeformat",
        &dir.join("bad.go"),
        &[("time", &dir.join("stub/time/time.go"))],
    );
    let messages = support::run_analyzer(timeformat_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("2006-02-01")));
}

#[test]
fn timeformat_allows_good_layout() {
    let dir = support::testdata("timeformat");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/timeformat/ok",
        &dir.join("ok.go"),
        &[("time", &dir.join("stub/time/time.go"))],
    );
    assert!(support::run_analyzer(timeformat_analyzer(), &pkg).is_empty());
}

#[test]
fn unmarshal_flags_non_pointer() {
    let dir = support::testdata("unmarshal");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/unmarshal",
        &dir.join("bad.go"),
        &[("encoding/json", &dir.join("stub/encoding/json/json.go"))],
    );
    let messages = support::run_analyzer(unmarshal_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("non-pointer")));
}

#[test]
fn unmarshal_allows_pointer() {
    let dir = support::testdata("unmarshal");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/unmarshal/ok",
        &dir.join("ok.go"),
        &[("encoding/json", &dir.join("stub/encoding/json/json.go"))],
    );
    assert!(support::run_analyzer(unmarshal_analyzer(), &pkg).is_empty());
}

#[test]
fn unsafeptr_flags_slice_header() {
    let dir = support::testdata("unsafeptr");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/unsafeptr",
        &dir.join("bad.go"),
        &[("reflect", &dir.join("stub/reflect/reflect.go"))],
    );
    let messages = support::run_analyzer(unsafeptr_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("unsafe.Pointer")));
}

#[test]
fn unsafeptr_allows_safe_code() {
    let dir = support::testdata("unsafeptr");
    let pkg = support::typecheck_pkg("example.com/govet/unsafeptr/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(unsafeptr_analyzer(), &pkg).is_empty());
}

#[test]
fn httpresponse_flags_defer_before_error_check() {
    let dir = support::testdata("httpresponse");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/httpresponse",
        &dir.join("bad.go"),
        &[("net/http", &dir.join("stub/net/http/http.go"))],
    );
    let messages = support::run_analyzer(httpresponse_analyzer(), &pkg);
    assert!(!messages.is_empty(), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("before checking for errors")));
}

#[test]
fn httpresponse_allows_checked_error() {
    let dir = support::testdata("httpresponse");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/httpresponse/ok",
        &dir.join("ok.go"),
        &[("net/http", &dir.join("stub/net/http/http.go"))],
    );
    assert!(support::run_analyzer(httpresponse_analyzer(), &pkg).is_empty());
}

#[test]
fn lostcancel_flags_discarded_cancel() {
    let dir = support::testdata("lostcancel");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/lostcancel",
        &dir.join("bad.go"),
        &[
            ("context", &dir.join("stub/context/context.go")),
            ("time", &dir.join("stub/time/time.go")),
        ],
    );
    let messages = support::run_analyzer(lostcancel_analyzer(), &pkg);
    assert!(
        messages.iter().filter(|m| m.contains("discarded")).count() >= 2,
        "expected discarded-cancel in FuncDecl and FuncLit, got {messages:?}"
    );}

/// The path shapes in `paths.go`. Positions and wording are pinned against
/// golangci-lint by `compat/golden/cases/govet`; this test guards the
/// shape of the answer (which functions, how many reports, whose name is in the
/// message) without needing golangci-lint on PATH.
#[test]
fn lostcancel_reports_uncovered_paths() {
    let dir = support::testdata("lostcancel");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/lostcancel/paths",
        &dir.join("paths.go"),
        &[
            ("context", &dir.join("stub/context/context.go")),
            ("time", &dir.join("stub/time/time.go")),
        ],
    );
    let messages = support::run_analyzer(lostcancel_analyzer(), &pkg);

    let not_all_paths = messages
        .iter()
        .filter(|m| m.ends_with("function is not used on all paths (possible context leak)"))
        .count();
    let reachable_return = messages
        .iter()
        .filter(|m| m.starts_with("this return statement may be reached without using the "))
        .count();
    let discarded = messages.iter().filter(|m| m.contains("discarded")).count();

    // 12 `leak*` functions report a pair, `leakDiscarded` reports once.
    assert_eq!(not_all_paths, 12, "{messages:#?}");
    assert_eq!(reachable_return, 12, "{messages:#?}");
    assert_eq!(discarded, 1, "{messages:#?}");
    assert_eq!(messages.len(), 25, "{messages:#?}");

    // The variable's own name goes into both messages, not a literal "cancel".
    assert!(
        messages
            .iter()
            .any(|m| m == "the kill function is not used on all paths (possible context leak)"),
        "{messages:#?}"
    );
    assert!(
        messages.iter().any(|m| m
            .starts_with("this return statement may be reached without using the kill var defined on line ")),
        "{messages:#?}"
    );
}

#[test]
fn lostcancel_allows_deferred_cancel() {
    let dir = support::testdata("lostcancel");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/lostcancel/ok",
        &dir.join("ok.go"),
        &[
            ("context", &dir.join("stub/context/context.go")),
            ("time", &dir.join("stub/time/time.go")),
        ],
    );
    assert!(support::run_analyzer(lostcancel_analyzer(), &pkg).is_empty());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod framepointer_tests {
    use std::path::PathBuf;

    use super::*;

    fn asm_fixture(name: &str) -> PathBuf {
        support::testdata("framepointer").join(name)
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn framepointer_flags_clobber_arm64() {
        let dir = support::testdata("framepointer");
        let pkg = support::typecheck_with_other_files(
            "example.com/govet/framepointer",
            &dir.join("asm.go"),
            &[asm_fixture("bad_arm64.s")],
            &[],
        );
        let messages = support::run_analyzer(framepointer_analyzer(), &pkg);
        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(
            messages
                .iter()
                .all(|m| m.contains("frame pointer is clobbered before saving"))
        );
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn framepointer_allows_saved_fp_arm64() {
        let dir = support::testdata("framepointer");
        let pkg = support::typecheck_with_other_files(
            "example.com/govet/framepointer/ok",
            &dir.join("asm.go"),
            &[asm_fixture("ok_arm64.s")],
            &[],
        );
        assert!(support::run_analyzer(framepointer_analyzer(), &pkg).is_empty());
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn framepointer_flags_clobber_amd64() {
        let dir = support::testdata("framepointer");
        let pkg = support::typecheck_with_other_files(
            "example.com/govet/framepointer",
            &dir.join("asm.go"),
            &[asm_fixture("bad_amd64.s")],
            &[],
        );
        let messages = support::run_analyzer(framepointer_analyzer(), &pkg);
        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(
            messages
                .iter()
                .all(|m| m.contains("frame pointer is clobbered before saving"))
        );
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn framepointer_allows_saved_fp_amd64() {
        let dir = support::testdata("framepointer");
        let pkg = support::typecheck_with_other_files(
            "example.com/govet/framepointer/ok",
            &dir.join("asm.go"),
            &[asm_fixture("ok_amd64.s")],
            &[],
        );
        assert!(support::run_analyzer(framepointer_analyzer(), &pkg).is_empty());
    }
}
