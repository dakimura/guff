mod support;

#[test]
fn finds_todo_without_author() {
    let _ = guff_plugin_example::FORCE_LINK;
    guff_plugin::clear_instances();
    let analyzers = guff_plugin::instantiate("example", &serde_yaml::Value::Null).unwrap();
    let pkg = support::typecheck_pkg(
        "example.com/todo",
        &support::testdata("todo").join("todo.go"),
    );
    let msgs = support::run_analyzer(analyzers[0], &pkg);
    assert!(
        msgs.iter().any(|m| m.contains("TODO comment has no author")),
        "expected TODO finding, got {msgs:?}"
    );
    assert_eq!(msgs.len(), 1, "TODO(alice) should be ignored: {msgs:?}");
}
