//! Tests for chunk-19: `Checker` error collection (`error`, `type_str`).

use guff_types::basic::BasicKind;
use guff_types::{Checker, Config};
use guff_types_errors::Code;

#[test]
fn errors_are_collected_in_order_and_first_err_is_set() {
    let mut c = Checker::new(Config::default());
    assert!(c.first_err.is_none());

    c.error(0, Code::IncompatibleAssign, "first problem");
    c.error(5, Code::InvalidIfaceAssign, "second problem");

    assert_eq!(c.errors.len(), 2);
    assert_eq!(c.errors[0].msg, "first problem");
    assert_eq!(c.errors[0].code, Code::IncompatibleAssign);
    assert_eq!(c.errors[1].msg, "second problem");
    assert_eq!(c.errors[1].pos, 5);

    // first_err holds the first message only.
    assert_eq!(c.first_err.as_deref(), Some("first problem"));
}

#[test]
fn invalid_syntax_tree_gets_prefix() {
    let mut c = Checker::new(Config::default());
    c.error(0, Code::InvalidSyntaxTree, "bad node");
    assert_eq!(c.errors[0].msg, "invalid syntax tree: bad node");
    // first_err captures the prefixed message.
    assert_eq!(
        c.first_err.as_deref(),
        Some("invalid syntax tree: bad node")
    );
}

#[test]
fn type_str_renders_predeclared_type() {
    let c = Checker::new(Config::default());
    let int = c.basic(BasicKind::Int);
    assert_eq!(c.type_str(int), "int");
}

#[test]
fn error_message_can_embed_type_str() {
    let mut c = Checker::new(Config::default());
    let s = c.basic(BasicKind::String);
    let name = c.type_str(s);
    c.error(
        0,
        Code::IncompatibleAssign,
        format!("cannot use value as {}", name),
    );
    assert_eq!(c.errors[0].msg, "cannot use value as string");
}
