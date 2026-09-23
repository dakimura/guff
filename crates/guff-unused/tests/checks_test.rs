mod support;

use guff_unused::analyzer;

#[test]
fn unused_flags_unexported_func() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/unused/basic", &dir.join("bad.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("is unused"));
}

/// `//lint:ignore U1000` marks an object used — honnef's own syntax, which
/// golangci-lint honours for `unused` as well. Only the directive's own line
/// (trailing) or the declaration under it (doc comment) is covered, and a
/// directive naming some other check is not this one.
#[test]
fn unused_honours_lint_ignore() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/unused/lintignore", &dir.join("lint_ignore.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages.iter().any(|m| m.contains("reportedVar")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("reportedFunc")),
        "{messages:?}"
    );
}

/// Function-local `type` and `const` declarations.
///
/// honnef's `seeScope` sees every object in every scope and `g.stmt`'s
/// `*ast.DeclStmt` arm calls the same `g.decl` the package level uses, so a
/// type or constant declared inside a function body is a candidate like any
/// other — only *variables* are exempt (`LocalVariablesAreUsed`). guff's
/// `unused` was package-level-only and reported none of these.
///
/// The row list is pinned in full rather than by `any(contains(…))`: the
/// interesting half of this fixture is what stays *silent* (a declaration owned
/// by an unused function, a blank-named type, a const group one member keeps
/// alive, a `//lint:ignore`d one), and a spot check cannot see those.
/// `compat/golden/cases/unused` gates the same file against golangci-lint
/// 2.12.2.
#[test]
fn unused_reports_function_local_types_and_constants() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/unused/locals", &dir.join("locals.go"));
    let mut messages = support::run_analyzer(analyzer(), &pkg);
    messages.sort();
    assert_eq!(
        messages,
        vec![
            "const c is unused".to_string(),
            "const four is unused".to_string(),
            "const three is unused".to_string(),
            "field unusedField is unused".to_string(),
            "func localTypeUnused is unused".to_string(),
            "func recv.M is unused".to_string(),
            "type Exported is unused".to_string(),
            "type alias is unused".to_string(),
            "type box is unused".to_string(),
            "type deep is unused".to_string(),
            "type holder is unused".to_string(),
            "type iface is unused".to_string(),
            "type inCase is unused".to_string(),
            "type inFunc is unused".to_string(),
            "type inIf is unused".to_string(),
            "type io is unused".to_string(),
            "type leaf is unused".to_string(),
            "type recv is unused".to_string(),
        ],
        "{messages:?}"
    );
}

/// `//lint:file-ignore U1000` covers the *file* it is written in, but upstream
/// does not stop at the file: an ignored `*types.TypeName` is marked used and
/// then every method of the named type is too, wherever it was declared
/// (`unused/unused.go`, "use methods and fields of ignored types"). And an
/// ignored object is a root, not a silenced report, so what it references stays
/// alive. nats-server needs both — the directive sits on
/// `jetstream_helpers_test.go`, `type cluster` with it, and the methods are
/// spread over the sibling `*_test.go` files.
#[test]
fn unused_file_ignore_reaches_methods_declared_elsewhere() {
    let dir = support::testdata("fileignore");
    let types = dir.join("types.go");
    let methods = dir.join("methods.go");
    let pkg = support::typecheck_pkg_files(
        "example.com/unused/fileignore",
        &[types.as_path(), methods.as_path()],
    );
    let messages = support::run_analyzer(analyzer(), &pkg);
    let mut got: Vec<&String> = messages.iter().collect();
    got.sort();
    // `(*cluster).inPlainFile` (method of an ignored type) and `keptAlive`
    // (reached only from an ignored function) must not be here.
    assert_eq!(messages.len(), 2, "{got:?}");
    for want in ["unusedMethod", "unusedFree"] {
        assert!(messages.iter().any(|m| m.contains(want)), "{got:?}");
    }
}

#[test]
fn unused_allows_referenced_func() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/unused/basic/ok", &dir.join("ok.go"));
    assert!(support::run_analyzer(analyzer(), &pkg).is_empty());
}

#[test]
fn unused_flags_unexported_type() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/unused/type", &dir.join("type_bad.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("unusedType is unused"));
}

#[test]
fn unused_const_group_marks_siblings_used() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/unused/const", &dir.join("const_group_ok.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn unused_const_group_with_exported_marks_unexported_siblings() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg(
        "example.com/unused/const_exported",
        &dir.join("const_group_exported_ok.go"),
    );
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert!(messages.is_empty(), "{messages:?}");
}

/// honnef's graph is reachability from a root, not a reference count: a call
/// written inside a function nothing calls does not keep its target alive.
#[test]
fn unused_does_not_let_dead_code_keep_its_callees_alive() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/unused/deadcycle", &dir.join("dead_cycle.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    let mut got: Vec<&String> = messages.iter().collect();
    got.sort();
    assert_eq!(messages.len(), 3, "{got:?}");
    for want in ["recompileAll", "update", "delete"] {
        assert!(messages.iter().any(|m| m.contains(want)), "{got:?}");
    }
    // `Reload` is exported, so everything it reaches stays used.
    for live in ["reachable", "alsoReachable"] {
        assert!(!messages.iter().any(|m| m.contains(live)), "{got:?}");
    }
}

#[test]
fn unused_flags_method_on_used_type() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/unused/method", &dir.join("method_bad.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("unusedMethod is unused"));
}

/// A generic receiver is printed with its type parameter list.
///
/// honnef names a method by its receiver *type*, and `types` prints
/// `holder[T]`, not `holder`. Dropping the list produced
/// `func (*holder).run is unused` where golangci-lint says
/// `func (*holder[T]).run is unused` — same finding, same line, different text,
/// which is the kind of difference only a check-level gate sees.
#[test]
fn unused_prints_generic_receiver_type_arguments() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg(
        "example.com/unused/genericmethod",
        &dir.join("generic_method_bad.go"),
    );
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert!(
        messages
            .iter()
            .any(|m| m == "func (*holder[T]).run is unused"),
        "pointer receiver keeps its type parameter: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m == "func pair[K, V].key is unused"),
        "value receiver keeps both, comma-separated: {messages:?}"
    );
}

#[test]
fn unused_keeps_interface_impl_methods() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/unused/iface", &dir.join("iface_ok.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("trulyUnused is unused"));
}

/// A generic sealing interface keeps its implementations alive — and the
/// neighbouring fixture's does not.
///
/// `unused` carries its own `implements` (unused/implements.go), in which an
/// interface method's **bare** type parameter binds to whatever the concrete
/// method uses (consistently, across the interface), while a type parameter
/// *inside* another type matches nothing. So `sigil(T)` is satisfied by both
/// `sigil(string)` and `sigil(int)`, and `generic_iface.go`'s
/// `list() ([]T, error)` is satisfied by no concrete `list`.
///
/// Matching interface methods by name cannot separate the two — the names are
/// the same on both sides — which is why guff resolves generic interfaces by
/// signature. Measured against golangci-lint 2.12.2 on both fixtures; the
/// golden gates them side by side.
#[test]
fn unused_resolves_generic_interfaces_by_signature() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg(
        "example.com/unused/ifaceinstance",
        &dir.join("iface_instance.go"),
    );
    let mut messages = support::run_analyzer(analyzer(), &pkg);
    messages.sort();
    assert_eq!(
        messages,
        vec!["field index is unused", "field index is unused"],
        "both `sigil` methods implement ResultRef[T] and stay: {messages:?}"
    );
}

/// (2.1) "named types use exported methods" is an *edge from the type*.
///
/// `g.readSelection(m, named)` keeps an exported method alive only while its
/// receiver type is alive. Treating the method as a root instead let it
/// resurrect the type, so a type nothing references vanished from the report
/// together with its methods — five findings on opentofu's
/// `basicComponentFactory`.
#[test]
fn unused_reports_exported_methods_of_unused_types() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg(
        "example.com/unused/exportedmethod",
        &dir.join("exported_method.go"),
    );
    let mut messages = support::run_analyzer(analyzer(), &pkg);
    messages.sort();
    assert_eq!(
        messages,
        vec![
            "func (*withExported).Only is unused",
            "func (*withUnexported).only is unused",
            "type withExported is unused",
            "type withUnexported is unused",
        ],
        "the used type keeps its exported method: {messages:?}"
    );
}

/// (1.5) "packages use init functions" has no receiver guard, so a *method*
/// named `init` is a root — and its callees ride along.
#[test]
fn unused_treats_a_method_named_init_as_a_root() {
    let dir = support::testdata("basic");
    let pkg =
        support::typecheck_pkg("example.com/unused/initmethod", &dir.join("init_method.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(
        messages,
        vec!["func (*Diff).initNever is unused"],
        "`init` and the `reset` it calls are both alive: {messages:?}"
    );
}

/// The struct-field half of `unused`, which guff did not model at all.
///
/// honnef makes a named struct type *own* its fields (`edgeKindOwn`): they are
/// candidates in their own right, but a field is reported only when its owner
/// type is used — otherwise the type is the finding and `colorAndQuieten`
/// silences what it owns. `type neverUsed` below is the one finding for its two
/// fields; `type plainInner` is reported *and* so is the embedded field that
/// names it, because (7.2) "fields use their types" is an edge from the field.
///
/// Exact list, not `any(contains(…))`: most of these messages differ only in a
/// name, and the two `deadCv` rows differ in nothing at all.
#[test]
fn unused_reports_struct_fields() {
    let dir = support::testdata("fields");
    let pkg = support::typecheck_pkg("example.com/unused/fields", &dir.join("fields.go"));
    let mut messages = support::run_analyzer(analyzer(), &pkg);
    messages.sort();
    assert_eq!(
        messages,
        vec![
            "field dead is unused",
            "field deadBlnk is unused",
            "field deadBox is unused",
            "field deadCv is unused",
            "field deadCv is unused",
            "field deadE is unused",
            "field deadF is unused",
            "field deadI is unused",
            "field deadIn is unused",
            "field deadIn3 is unused",
            "field deadLoc is unused",
            "field deadNode is unused",
            "field deadOut is unused",
            "field deadOut2 is unused",
            "field deadOut3 is unused",
            "field deadP is unused",
            "field deadProm is unused",
            "field deadQ is unused",
            "field f is unused",
            // The only field of the four defined-type shapes that survives:
            // `DefinedOver2{keyedLive: 1}` writes one of its two fields, and
            // the defined type owns neither — upstream reaches them through
            // the struct behind it.
            "field keyedDead is unused",
            "field m is unused",
            "field mumbler2 is unused",
            "field plainInner is unused",
            "field private is unused",
            "field shared is unused",
            "field unrTag is unused",
            "field unread is unused",
            "field unset is unused",
            "func deadReader is unused",
            "func mumbler2.mumble2 is unused",
            "type mumbler2 is unused",
            "type neverUsed is unused",
            "type plainInner is unused",
        ],
        "{messages:?}"
    );
}

/// `linters.settings.unused`. avalanchego turns `field-writes-are-uses` off and
/// `post-statements-are-reads` on; guff read neither, so every field the repo
/// only ever writes to stayed silent and the `//nolint:unused` over one of them
/// became an unused directive (COMPAT-HARDENING §4).
///
/// With both options at their defaults nothing here is reported: a write is a
/// use, which is what makes the fixture a control as well as a subject.
#[test]
fn unused_settings_default_reports_no_write_only_field() {
    let dir = support::testdata("fieldwrites");
    let pkg = support::typecheck_pkg(
        "example.com/unused/fieldwrites",
        &dir.join("fieldwrites.go"),
    );
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages, Vec::<String>::new(), "{messages:?}");
}

/// `field-writes-are-uses: false`. Exact list, not `any(contains(…))`: seven of
/// the eleven rows differ in nothing at all, so a count is the only thing that
/// distinguishes "every shape reported" from "one shape reported seven times".
#[test]
fn unused_settings_field_writes_are_uses_false() {
    let dir = support::testdata("fieldwrites");
    let pkg = support::typecheck_pkg(
        "example.com/unused/fieldwrites",
        &dir.join("fieldwrites.go"),
    );
    let mut messages = support::run_analyzer_with_settings(
        analyzer(),
        &pkg,
        "unused",
        guff_unused::Options {
            field_writes_are_uses: false,
            ..guff_unused::Options::default()
        },
    );
    messages.sort();
    assert_eq!(
        messages,
        vec![
            // `v.a.b = "x"` — `a` is read as part of `node.X`, `b` is not.
            "field b is unused",
            // `v.n++`, which `post-statements-are-reads` would spare.
            "field n is unused",
            // A promoted write reads only `v`.
            "field pin is unused",
            // keyed literal, unkeyed literal, `=`, `++`'s neighbour `+=`,
            // multi-assign, `(*v).written =`, `for v.written = range`.
            "field written is unused",
            "field written is unused",
            "field written is unused",
            "field written is unused",
            "field written is unused",
            "field written is unused",
            "field written is unused",
            "type pin is unused",
        ],
        "{messages:?}"
    );
}

/// `post-statements-are-reads: true` on top, which is the pair avalanchego
/// sets. It changes exactly one row: `v.n++` becomes a read as well as a write.
/// honnef's (9.7) "variable *reads* use variables, writes do not" and (4.9)
/// "functions use package-level variables they assign to iff in tests".
///
/// A package-level variable that is only ever assigned to is unused. The one
/// exception is a global **declared in a `_test.go` file** — benchmark sinks —
/// and the rule asks about the declaring file, not the writing one, which is
/// why `writtenFromTest` is reported although a benchmark assigns to it.
///
/// Exact list: seven of these messages differ only in a name.
#[test]
fn unused_reports_a_global_that_is_only_written() {
    let dir = support::testdata("globalwrites");
    let src = dir.join("globalwrites.go");
    let test = dir.join("globalwrites_test.go");
    let pkg = support::typecheck_pkg_files(
        "example.com/unused/globalwrites",
        &[src.as_path(), test.as_path()],
    );
    let mut messages = support::run_analyzer(analyzer(), &pkg);
    messages.sort();
    assert_eq!(
        messages,
        vec![
            "var compoundAssigned is unused",
            "var incremented is unused",
            "var neverTouched is unused",
            "var rangeKey is unused",
            "var writtenFromTest is unused",
            "var writtenInFunc is unused",
            "var writtenInInit is unused",
        ],
        "{messages:?}"
    );
}

/// `post-statements-are-reads` makes `x++` a read, so `incremented` survives —
/// the one shape the default config cannot tell from a plain write.
#[test]
fn unused_post_statements_are_reads_keeps_an_incremented_global() {
    let dir = support::testdata("globalwrites");
    let src = dir.join("globalwrites.go");
    let test = dir.join("globalwrites_test.go");
    let pkg = support::typecheck_pkg_files(
        "example.com/unused/globalwrites",
        &[src.as_path(), test.as_path()],
    );
    let mut messages = support::run_analyzer_with_settings(
        analyzer(),
        &pkg,
        "unused",
        guff_unused::Options {
            field_writes_are_uses: true,
            post_statements_are_reads: true,
        },
    );
    messages.sort();
    assert_eq!(
        messages,
        vec![
            "var compoundAssigned is unused",
            "var neverTouched is unused",
            "var rangeKey is unused",
            "var writtenFromTest is unused",
            "var writtenInFunc is unused",
            "var writtenInInit is unused",
        ],
        "{messages:?}"
    );
}

#[test]
fn unused_settings_post_statements_are_reads() {
    let dir = support::testdata("fieldwrites");
    let pkg = support::typecheck_pkg(
        "example.com/unused/fieldwrites",
        &dir.join("fieldwrites.go"),
    );
    let mut messages = support::run_analyzer_with_settings(
        analyzer(),
        &pkg,
        "unused",
        guff_unused::Options {
            field_writes_are_uses: false,
            post_statements_are_reads: true,
        },
    );
    messages.sort();
    assert_eq!(
        messages,
        vec![
            "field b is unused",
            "field pin is unused",
            "field written is unused",
            "field written is unused",
            "field written is unused",
            "field written is unused",
            "field written is unused",
            "field written is unused",
            "field written is unused",
            "type pin is unused",
        ],
        "{messages:?}"
    );
}
