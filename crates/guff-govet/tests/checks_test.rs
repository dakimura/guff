mod support;

use guff_govet::{
    appends_analyzer, assign_analyzer, atomic_analyzer, bools_analyzer, buildtag_analyzer,
    cgocall_analyzer, composites_analyzer, copylocks_analyzer, defers_analyzer,
    directive_analyzer, errorsas_analyzer, framepointer_analyzer, httpresponse_analyzer,
    hostport_analyzer, ifaceassert_analyzer, inline_analyzer, loopclosure_analyzer, lostcancel_analyzer,
    nilfunc_analyzer, nilness_analyzer, printf_analyzer, shift_analyzer, sigchanyzer_analyzer, slog_analyzer,
    stdmethods_analyzer, stringintconv_analyzer, structtag_analyzer, tests_analyzer,
    fieldalignment_analyzer, timeformat_analyzer, unmarshal_analyzer, unreachable_analyzer,
    unsafeptr_analyzer,
    testinggoroutine_analyzer, unusedresult_analyzer, waitgroup_analyzer,
};
use guff_types::Config;

/// `fieldalignment` reports at the `struct` keyword — `node.Pos()` of an
/// `*ast.StructType` — which is neither the type name nor the `{`, and the
/// message carries no hint of where it landed. So this pins `(line, column)`
/// as well as the message, and pins the **count**: thirty struct types are
/// written in the fixture and fourteen of them are reported. The sixteen
/// silent ones are the point of the other half of the file — an analyzer that
/// reported every struct would still pass an `any(contains(…))` assertion.
#[test]
fn fieldalignment_reports_size_and_pointer_bytes_at_the_struct_keyword() {
    let dir = support::testdata("fieldalignment");
    let pkg = support::typecheck_pkg("example.com/govet/fieldalignment", &dir.join("bad.go"));
    let fset = pkg.fset.clone().expect("fixture has a FileSet");
    let mut got: Vec<(i64, i64, String)> = support::run_analyzer_diagnostics(
        fieldalignment_analyzer(),
        &pkg,
    )
    .into_iter()
    .map(|d| {
        let p = fset.position(guff::position::Pos(d.pos as i64));
        (p.line, p.column, d.message)
    })
    .collect();
    got.sort();

    let size = |line, col, from, to| (line, col, format!("struct of size {from} could be {to}"));
    let ptrs = |line, col, from, to| {
        (
            line,
            col,
            format!("struct with {from} pointer bytes could be {to}"),
        )
    };
    assert_eq!(
        got,
        vec![
            size(16, 24, 24, 16),  // bool, int64, bool
            size(23, 20, 40, 32),  // an array's element alignment counts
            size(30, 19, 24, 16),  // tags do not move a field
            size(36, 22, 32, 24),  // complex128
            size(45, 4, 24, 16),   // the anonymous struct, at its own `struct`
            ptrs(55, 23, 16, 8),   // uint32 then string
            ptrs(61, 20, 24, 16),  // string then *uint32
            ptrs(66, 21, 24, 16),  // an array of pointers
            size(74, 20, 32, 24),  // an interface is two words: a size finding
            ptrs(80, 14, 24, 16),  // any
            ptrs(85, 16, 16, 8),   // a slice
            ptrs(90, 22, 32, 24),  // map, chan, func
            ptrs(97, 17, 16, 8),   // bool, string, int64
            ptrs(105, 25, 24, 16), // a type parameter constrained by `any`
        ],
        "fieldalignment findings"
    );
}

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
fn printf_names_the_callee_in_full_and_has_a_no_directives_branch() {
    // Two defects, both visible on one shape. A format string with no `%` in it
    // is upstream's own branch, before any parsing: it reports the leftover
    // arguments at the **first argument after the format**, with its own
    // wording. guff fell through to the arity check — a different message at a
    // different position, so the two tools disagreed on a shape they both meant
    // to report (velero's `p.log.Errorf("error parsing operation ID's
    // StartedTime", …)`).
    //
    // And the name is `types.Func.FullName()`, so a method carries its
    // receiver. guff used package-path-plus-name, which for a method is
    // `logrus.Errorf` — a name Go never prints, and one that collides with the
    // package-level function of the same name. The table is keyed on those full
    // names, and picking up upstream's entries brought `Logf` with it, which
    // guff's short-name heuristic never had.
    let dir = support::testdata("printf");
    let fmt_stub = dir.join("stub/fmt/print.go");
    let log_stub = dir.join("stub/log/log.go");
    let testing_stub = dir.join("stub/testing/testing.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/printf/noverbs",
        &dir.join("noverbs_unit.go"),
        &[
            ("fmt", &fmt_stub),
            ("log", &log_stub),
            ("testing", &testing_stub),
        ],
    );
    let messages = support::run_analyzer(printf_analyzer(), &pkg);
    assert_eq!(
        messages,
        vec![
            "fmt.Printf call has arguments but no formatting directives",
            "fmt.Errorf call has arguments but no formatting directives",
            "fmt.Sprintf call has arguments but no formatting directives",
            "fmt.Printf call has arguments but no formatting directives",
            "fmt.Printf call needs 1 arg but has 2 args",
            "(*log.Logger).Printf call has arguments but no formatting directives",
            "(*testing.common).Errorf call has arguments but no formatting directives",
            "(*testing.common).Logf call has arguments but no formatting directives",
            "(*testing.common).Fatalf call has arguments but no formatting directives",
        ],
        "{messages:?}"
    );
}

#[test]
fn printf_deduces_wrappers_instead_of_guessing_from_the_name() {
    // guff decided a call was printf-like by looking at the callee's base name
    // (`Printf`/`Sprintf`/`Fprintf`/`Errorf`/`Fatalf`/`Panicf`). Upstream never
    // does that: outside its allowlist a function is printf-like only if its
    // body forwards `args...` to something that already is. A two-package
    // reproducer put the two tools' answers side by side and they were
    // *disjoint* — six findings against three, nothing in common. The three
    // guff had were methods whose names end in `f` and whose bodies forward
    // nothing; the six it lacked were real wrappers it could not name.
    //
    // Every shape below was measured against golangci-lint 2.12.2 first. The
    // list is exhaustive and ordered by position, so a shape that stops firing
    // fails here rather than quietly leaving the grid.
    //
    // The package-level names come out bare here (`wrapf`, not
    // `example.com/govet/printf/wrappers.wrapf`): this harness type-checks a
    // single file without stamping the import path onto the package, and the
    // end-to-end runs above print the qualified form. Only the *shape* of each
    // name matters below — which object was blamed, and whether it carries a
    // receiver.
    let dir = support::testdata("printf");
    let fmt_stub = dir.join("stub/fmt/print.go");
    let testing_stub = dir.join("stub/testing/testing.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/printf/wrappers",
        &dir.join("wrappers.go"),
        &[("fmt", &fmt_stub), ("testing", &testing_stub)],
    );
    let messages = support::run_analyzer(printf_analyzer(), &pkg);
    assert_eq!(
        messages,
        vec![
            // a wrapper deduced from its body; `fmt.Printf` is not KindErrorf
            "wrapf does not support error-wrapping directive %w",
            // forwarding without `...`, named by the callee's kind
            "missing ... in args forwarded to printf-like function",
            // an unknown verb reached through the wrapper
            "wrapf format %z has unknown verb z",
            // a function literal is named by the variable that holds it
            "litf format %z has unknown verb z",
            // and the kind travels back along a chain of wrappers
            "hop1 format %z has unknown verb z",
            // `(*testing.common).Errorf` is KindPrintf, so `%w` is a diagnostic
            "(*testing.common).Errorf does not support error-wrapping directive %w",
            // an unformatted wrapper says "print-like"
            "missing ... in args forwarded to print-like function",
            "sprintfWrapper format %z has unknown verb z",
            // a literal assigned to a struct field is named by the field
            "logf format %z has unknown verb z",
        ],
        "{messages:?}"
    );
}

#[test]
fn printf_does_not_see_a_wrapper_declared_in_another_package() {
    // The one shape in the wrapper grid where the two tools disagree, pinned so
    // that it is a recorded gap rather than a surprise. Upstream exports an
    // `isWrapper` object fact from `sub` and reports `format %z has unknown
    // verb z` on the call; guff analyses only the packages being linted and
    // `printf` advertises no facts, so it stays silent. Measured against
    // golangci-lint 2.12.2 on a two-package module.
    let dir = support::testdata("printf");
    let fmt_stub = dir.join("stub/fmt/print.go");
    let sub_stub = dir.join("stub/sub/sub.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/printf/imported",
        &dir.join("wrappers_imported.go"),
        &[
            ("fmt", &fmt_stub),
            ("example.com/govet/printf/sub", &sub_stub),
        ],
    );
    assert_eq!(support::run_analyzer(printf_analyzer(), &pkg), Vec::<String>::new());
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

/// An explicit index binds to the position that absorbs it, each `*` operand
/// must be an int, a malformed directive is the only thing reported for its
/// format string, and only the first mistake in a format string is reported.
#[test]
fn printf_binds_indexes_to_stars_and_verbs() {
    let dir = support::testdata("printf");
    let stub = dir.join("stub/fmt/print.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/printf/indexes",
        &dir.join("indexes.go"),
        &[("fmt", &stub)],
    );
    let messages = support::run_analyzer(printf_analyzer(), &pkg);
    assert_eq!(messages.len(), 16, "{messages:?}");
    // rclone's line: the width is argument 2, the string argument 1, so
    // nothing is wrong with it — only the `%d` twin below is reported.
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("%[2]*[1]"))
            .collect::<Vec<_>>(),
        vec![&"fmt.Printf format %[2]*[1]d has arg str of wrong type string".to_string()],
        "{messages:?}"
    );
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("as argument of *"))
            .count(),
        4,
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.ends_with("format has invalid argument index [999999999999]")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.ends_with("format %[3d b %s is missing closing ]")),
        "{messages:?}"
    );
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
    // The callee side of this comparison is the *host toolchain* version, and
    // `version_compare` only reads major.minor. A literal caller version made
    // the test a function of which Go the machine had: on go1.24.x a "1.24.3"
    // caller compared equal, the diagnostic never fired, and the test failed.
    // Derive a caller one minor below the toolchain so the mismatch holds on
    // any Go.
    let toolchain = guff_analysis::code::toolchain_go_version();
    let caller = one_minor_below(&toolchain);
    let pkg = support::with_go_version(
        support::typecheck_with_deps(
            "example.com/govet/inline_ioutil",
            &dir.join("bad.go"),
            &[("io/ioutil", &stub)],
        ),
        &caller,
    );
    let messages = support::run_analyzer(inline_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(
        messages[0].starts_with("cannot inline call to ioutil.TempDir (declared using go"),
        "{messages:?}"
    );
    assert!(
        messages[0].contains(&format!("into a file using go{caller}")),
        "{messages:?}"
    );
}

/// Upstream gates on `versions.Before(caller, callee)`, so a caller at the
/// toolchain's own version is *not* a mismatch. Without this the check could
/// start firing on every `io/ioutil` call and nothing in the suite would say so.
#[test]
fn inline_allows_ioutil_at_the_toolchain_version() {
    let dir = support::testdata("inline_ioutil");
    let stub = dir.join("stub/io/ioutil/ioutil.go");
    let pkg = support::with_go_version(
        support::typecheck_with_deps(
            "example.com/govet/inline_ioutil",
            &dir.join("bad.go"),
            &[("io/ioutil", &stub)],
        ),
        &guff_analysis::code::toolchain_go_version(),
    );
    assert!(
        support::run_analyzer(inline_analyzer(), &pkg).is_empty(),
        "caller at the toolchain version must not be reported"
    );
}

/// `go1.26.5` -> `1.25.0`. Panics rather than guessing: every Go this crate
/// builds under is well past 1.0, so a version that will not parse means the
/// toolchain probe itself broke, which is worth failing loudly.
fn one_minor_below(toolchain: &str) -> String {
    let v = toolchain.strip_prefix("go").unwrap_or(toolchain);
    let mut parts = v.split('.');
    let major: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("unparsable toolchain version {toolchain:?}"));
    let minor: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("unparsable toolchain version {toolchain:?}"));
    assert!(minor >= 1, "toolchain {toolchain:?} has no earlier minor");
    format!("{major}.{}.0", minor - 1)
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

/// A target whose underlying type is the empty interface is always allowed.
///
/// Upstream: `types.Identical(t.Underlying(), anyType)` — "a target of any is
/// always allowed, since it often indicates a value forwarded from another
/// source". `any` is an alias for `interface{}`, so the test is structural and
/// a *named* empty interface passes too. guff compared the printed name
/// against "any", which never matched (the underlying prints as
/// `interface{}`), so every `func As(err error, target any) bool { return
/// errors.As(err, target) }` wrapper was a finding — thanos has one.
#[test]
fn errorsas_allows_any_target() {
    let dir = support::testdata("errorsas");
    let pkg = support::typecheck_pkg("example.com/govet/errorsas/ok", &dir.join("ok.go"));
    let messages = support::run_analyzer(errorsas_analyzer(), &pkg);
    assert!(
        messages.is_empty(),
        "any / interface{{}} / a named empty interface are all allowed: {messages:?}"
    );
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
    let pkg = support::typecheck_with_deps(
        "example.com/govet/unusedresult",
        &dir.join("bad.go"),
        &unusedresult_stub_refs(&unusedresult_stubs(&dir)),
    );
    let messages = support::run_analyzer(unusedresult_analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("errors.New"));
}

/// The stdlib packages the two `unusedresult` fixtures import, as stubs. The
/// analyzer keys on the callee's **package path**, so the check needs packages
/// with paths rather than bare identifiers — the very thing the defect was
/// about.
fn unusedresult_stubs(dir: &std::path::Path) -> Vec<(&'static str, std::path::PathBuf)> {
    ["errors", "fmt", "maps", "slices", "sort"]
        .into_iter()
        .map(|p| (p, dir.join("stub").join(p).join(format!("{p}.go"))))
        .collect()
}

fn unusedresult_stub_refs<'a>(
    stubs: &'a [(&'static str, std::path::PathBuf)],
) -> Vec<(&'static str, &'a std::path::Path)> {
    stubs.iter().map(|(p, path)| (*p, path.as_path())).collect()
}

#[test]
fn unusedresult_matches_the_package_path_and_the_string_methods() {
    // Upstream resolves the callee and keys on `{fn.Pkg().Path(), fn.Name()}`.
    // guff matched the identifier written before the dot, so
    // `github.com/pkg/errors.New` read as `errors.New` and was reported, while
    // the real one imported under a name was not (velero, both halves).
    //
    // Its table also held ten entries where upstream's list holds sixty — the
    // `fmt.Append*`, `maps.*` and `slices.*` families were missing entirely —
    // and there was no *method* branch at all: `Error` and `String`, and only
    // when the signature is identical to `func() string`.
    let dir = support::testdata("unusedresult");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/unusedresult",
        &dir.join("wider.go"),
        &unusedresult_stub_refs(&unusedresult_stubs(&dir)),
    );
    let messages = support::run_analyzer(unusedresult_analyzer(), &pkg);
    // Nine package-level calls and three methods; `Describe`, `Errorf(1)` and
    // the call through a func-typed variable say nothing.
    assert_eq!(messages.len(), 12, "{messages:?}");
    for want in [
        "result of errors.New call not used",
        "result of fmt.Append call not used",
        "result of fmt.Appendf call not used",
        "result of fmt.Appendln call not used",
        "result of maps.Keys call not used",
        "result of maps.Clone call not used",
        "result of slices.Clone call not used",
        "result of slices.Contains call not used",
        "result of sort.Reverse call not used",
        // The receiver is written with a nil qualifier — the package path, which
        // the golden case shows in full as
        // `(example.com/govet/unusedresult/wider.stringer)`; this harness
        // type-checks one file with no module path, so the name stands alone.
        "result of (stringer).String call not used",
        "result of (stringer).Error call not used",
        "result of (error).Error call not used",
    ] {
        assert!(messages.iter().any(|m| m == want), "{want}: {messages:?}");
    }
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
    assert_eq!(messages.len(), 4, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("not exported")), "{messages:?}");
    // Tag options are not part of the name, so `a,omitempty` collides with `a`.
    assert!(
        messages
            .iter()
            .any(|m| m.contains(r#"struct field A2 repeats json tag "a" also at bad.go:10"#)),
        "{messages:?}"
    );
    // XML attributes are their own namespace, and the key says so.
    assert!(
        messages.iter().any(
            |m| m.contains(r#"struct field Kind2 repeats xml attribute tag "kind" also at bad.go:"#)
        ),
        "{messages:?}"
    );
    // `XMLName` is exempt: nothing reported for XMLNameIsExempt.
    assert!(
        !messages.iter().any(|m| m.contains("ticketing-milestone")),
        "{messages:?}"
    );
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

/// `nilness` is the only SSA-based analyzer in `guff-govet`, and every one of
/// its report categories is a distinct instruction shape, so this pins the
/// **exact set** of (line, column, message) triples. The columns matter as
/// much as the messages: three of the shapes here (`*p`, `<-c`, and the
/// implicit `&s[i]` of a `for range`) only report at the right column because
/// guff-ssa now carries the source position through `emitLoad`, the unary
/// operator, and `rangeIndexed` — before that they were reported at the wrong
/// place, or (position 0) not at all.
///
/// The fixture also holds ten shapes that must stay silent — a static method
/// call on a nil receiver, `len` and index reads of a nil map/slice/chan, a
/// zero-valued struct passed to `panic`, a comma-ok assertion to a bare
/// interface, a reloaded struct field, the pruned arm of a tautology, and a
/// `MakeInterface` of an unconstrained type parameter. Measured against
/// golangci-lint 2.12.2 with `govet.enable: [nilness]`: 28 findings, and this
/// list is that output.
#[test]
fn nilness_reports_every_category_at_upstreams_position() {
    let dir = support::testdata("nilness");
    let pkg = support::typecheck_pkg("example.com/govet/nilness", &dir.join("bad.go"));
    let fset = pkg.fset.clone().expect("fixture has a FileSet");
    let mut got: Vec<(i64, i64, String)> =
        support::run_analyzer_diagnostics(nilness_analyzer(), &pkg)
            .into_iter()
            .map(|d| {
                let p = fset.position(guff::position::Pos(d.pos as i64));
                (p.line, p.column, d.message)
            })
            .collect();
    got.sort();

    let at = |line: i64, col: i64, msg: &str| (line, col, msg.to_string());
    assert_eq!(
        got,
        vec![
            at(20, 7, "tautological condition: nil == nil"),
            at(27, 7, "tautological condition: non-nil != nil"),
            at(34, 7, "impossible condition: non-nil == nil"),
            at(41, 8, "impossible condition: nil != nil"),
            at(49, 8, "tautological condition: nil == nil"),
            at(59, 7, "impossible condition: non-nil == nil"),
            at(66, 7, "impossible condition: non-nil == nil"),
            at(75, 12, "nil dereference in field selection"),
            at(82, 10, "nil dereference in load"),
            at(89, 3, "nil dereference in store"),
            at(95, 4, "nil dereference in map update"),
            at(101, 3, "range over nil map"),
            at(109, 3, "receive from nil channel"),
            at(115, 5, "send to nil channel"),
            at(121, 11, "index of nil slice"),
            at(128, 21, "range of nil slice"),
            at(136, 11, "nil dereference in array index operation"),
            at(143, 21, "nil dereference in array index operation"),
            at(151, 11, "nil dereference in slice operation"),
            at(158, 12, "nil dereference in type assertion"),
            at(165, 6, "nil dereference in dynamic method call"),
            at(171, 4, "nil dereference in dynamic function call"),
            at(177, 3, "nil dereference in dynamic function call"),
            at(178, 3, "nil dereference in dynamic function call"),
            at(195, 7, "panic with nil value"),
            at(209, 12, "nil dereference in field selection"),
            at(232, 6, "nil dereference in dynamic method call"),
            at(278, 7, "impossible condition: non-nil == nil"),
        ],
    );
}

/// The IR does not contain the statements after a call that cannot return.
/// `buildssa` hands go/ssa the `ctrlflow` no-return predicate, `emitCall` puts
/// a `Panic` behind such a call and starts an unreachable block, and
/// `deleteUnreachableBlocks` removes the rest — so by the time any analyzer
/// runs, the join block below has a single live predecessor and `err` is
/// provably nil in it.
///
/// nilness is the visible consequence, so it is what this asserts. The fixture
/// also holds the two shapes that must stay silent: the same code with a call
/// that *does* return (an implementation that cut after every call would still
/// pass without it), and a use after a branch that aborts, which is still
/// reachable. Measured against golangci-lint 2.12.2: one finding, and this is
/// it.
#[test]
fn nilness_sees_the_ir_cut_after_a_call_that_cannot_return() {
    let dir = support::testdata("nilness");
    let stub = dir.join("stub/log/log.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/nilness/noreturn",
        &dir.join("noreturn.go"),
        &[("log", &stub)],
    );
    let fset = pkg.fset.clone().expect("fixture has a FileSet");
    let got: Vec<(i64, i64, String)> =
        support::run_analyzer_diagnostics(nilness_analyzer(), &pkg)
            .into_iter()
            .map(|d| {
                let p = fset.position(guff::position::Pos(d.pos as i64));
                (p.line, p.column, d.message)
            })
            .collect();
    assert_eq!(
        got,
        vec![(22, 9, "impossible condition: nil != nil".to_string())],
    );
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
    // Six, not one: the package-level call, the two http.Client methods, the
    // two blocks nested in a loop and a func literal, and the selector whose
    // root is `h`. Anything less means the walk stopped somewhere.
    assert_eq!(messages.len(), 6, "{messages:?}");
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.as_str() == "using resp before checking for errors")
            .count(),
        5,
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m == "using h before checking for errors"),
        "{messages:?}"
    );
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

#[test]
fn testinggoroutine_flags_forbidden_calls_from_goroutines() {
    let dir = support::testdata("testinggoroutine");
    let stub = dir.join("stub/testing/testing.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/testinggoroutine",
        &dir.join("bad_test.go"),
        &[("testing", &stub)],
    );
    let messages = support::run_analyzer(testinggoroutine_analyzer(), &pkg);
    // Every shape of region: the go statement itself, a literal, a variable
    // holding a literal, a function of this package, and (*testing.B).
    assert_eq!(messages.len(), 8, "{messages:?}");
    assert!(messages
        .iter()
        .any(|m| m.contains("call to (*testing.T).Fatal from a non-test goroutine")));
    // The identifier that reached the region is named, and so is the method.
    assert!(messages
        .iter()
        .any(|m| m.contains("(fn calls (*testing.T).FailNow)")));
    assert!(messages
        .iter()
        .any(|m| m.contains("(helper calls (*testing.T).Fatal)")));
    assert!(messages
        .iter()
        .any(|m| m.contains("call to (*testing.B).Fatal")));
}

#[test]
fn testinggoroutine_leaves_the_tests_own_goroutine_alone() {
    let dir = support::testdata("testinggoroutine");
    let stub = dir.join("stub/testing/testing.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/testinggoroutine/ok",
        &dir.join("ok_test.go"),
        &[("testing", &stub)],
    );
    // Includes the two shapes that are easy to get wrong: Errorf (no Goexit)
    // inside a goroutine, and a t.Fatal inside a subtest nested in one — the
    // subtest region claims it, so upstream says nothing.
    let messages = support::run_analyzer(testinggoroutine_analyzer(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

/// The two rules that decide whether printf looks at an operand at all.
///
/// **`isFormatter`.** A value that might format itself makes every other check
/// meaningless, so upstream skips the operand — verb *and* type. Its test is
/// generous: any interface that is not a type parameter counts, because the
/// dynamic value could implement `fmt.Formatter`. That is the whole reason
/// `t.Fatalf("err %r", err)` is silent while `fmt.Sprintf("s %y", s)` is
/// reported, and guff had been reporting both.
///
/// `%w` is exempt from the test — every `%w` operand is an interface, so
/// applying it there would delete the error-wrapping diagnostics outright.
///
/// **Byte arrays.** `[N]byte` prints like a string for the string verbs, "same
/// as slice" upstream — and **byte only**: `[]rune` under `%s` is a finding,
/// because `fmt` prints it as a list of int32.
#[test]
fn printf_skips_operands_that_might_format_themselves() {
    let dir = support::testdata("printf");
    let stub = dir.join("stub/fmt/print.go");
    let pkg = support::typecheck_with_deps(
        "example.com/govet/printf/formatter",
        &dir.join("formatter.go"),
        &[("fmt", &stub)],
    );
    let mut messages = support::run_analyzer(printf_analyzer(), &pkg);
    messages.sort();

    // Exactly the five `// fires` functions. The eight silent ones are what
    // this pins: four operands that might be formatters, three byte-array or
    // byte-slice shapes, and the `%w` that really is wrapping.
    assert_eq!(
        messages,
        vec![
            "fmt.Sprintf does not support error-wrapping directive %w".to_string(),
            "fmt.Sprintf format %s has arg a of wrong type [3]int".to_string(),
            "fmt.Sprintf format %s has arg r of wrong type []rune".to_string(),
            "fmt.Sprintf format %y has unknown verb y".to_string(),
            "fmt.Sprintf format %y has unknown verb y".to_string(),
        ],
        "{messages:?}"
    );
}
