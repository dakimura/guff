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
