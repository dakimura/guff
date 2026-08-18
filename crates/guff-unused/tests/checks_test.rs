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
