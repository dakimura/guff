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
fn unused_flags_method_on_used_type() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/unused/method", &dir.join("method_bad.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("unusedMethod is unused"));
}

#[test]
fn unused_keeps_interface_impl_methods() {
    let dir = support::testdata("basic");
    let pkg = support::typecheck_pkg("example.com/unused/iface", &dir.join("iface_ok.go"));
    let messages = support::run_analyzer(analyzer(), &pkg);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("trulyUnused is unused"));
}
