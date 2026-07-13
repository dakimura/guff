//! Tests for the embedded-field validity check (chunk 61, `struct.go` port).
//!
//! spec: "An embedded type must be specified as a type name T or as a pointer
//! to a non-interface type name *T, and T itself may not be a pointer type."

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_types::{Checker, Config};
use guff_types_errors::Code;

fn parse(src: &str) -> guff::ast::File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

fn check_src(src: &str) -> Checker {
    let mut check = Checker::new(Config::default());
    check.check_files(vec![parse(src)]);
    check
}

fn has_code(check: &Checker, code: Code) -> bool {
    check.errors.iter().any(|e| e.code == code)
}

#[test]
fn embedded_pointer_to_basic_is_ok() {
    // *int: pointer to a non-interface type name — valid.
    let check = check_src("package p\ntype T struct{ *int }\n");
    assert!(
        !has_code(&check, Code::InvalidPtrEmbed),
        "unexpected embed error: {:?}",
        check.errors
    );
}

#[test]
fn embedded_interface_is_ok() {
    // Embedding an interface value in a struct is allowed.
    let check = check_src("package p\ntype I interface{}\ntype T struct{ I }\n");
    assert!(
        !has_code(&check, Code::InvalidPtrEmbed) && !has_code(&check, Code::MisplacedTypeParam),
        "unexpected embed error: {:?}",
        check.errors
    );
}

#[test]
fn embedded_named_pointer_type_is_error() {
    // type P *int; embedding P fails: T itself may not be a pointer type.
    let check = check_src("package p\ntype P *int\ntype T struct{ P }\n");
    assert!(
        has_code(&check, Code::InvalidPtrEmbed),
        "expected InvalidPtrEmbed: {:?}",
        check.errors
    );
}

#[test]
fn embedded_pointer_to_interface_is_error() {
    // *I where I is an interface — pointer to an interface is invalid.
    let check = check_src("package p\ntype I interface{}\ntype T struct{ *I }\n");
    assert!(
        has_code(&check, Code::InvalidPtrEmbed),
        "expected InvalidPtrEmbed: {:?}",
        check.errors
    );
}

#[test]
fn embedded_unsafe_pointer_is_error() {
    // unsafe.Pointer is treated like a regular pointer — cannot be embedded.
    let check = check_src("package p\nimport \"unsafe\"\ntype T struct{ unsafe.Pointer }\n");
    assert!(
        has_code(&check, Code::InvalidPtrEmbed),
        "expected InvalidPtrEmbed: {:?}",
        check.errors
    );
}

#[test]
fn embedded_type_parameter_is_error() {
    // A (pointer to a) type parameter cannot be embedded.
    let check = check_src("package p\ntype T[P any] struct{ P }\n");
    assert!(
        has_code(&check, Code::MisplacedTypeParam),
        "expected MisplacedTypeParam: {:?}",
        check.errors
    );
}
