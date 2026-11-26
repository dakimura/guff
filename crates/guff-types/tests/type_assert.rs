//! Tests for type-assertion expressions `x.(T)` (chunk 34a).
//!
//! Driven through the full `check_files` pipeline: a function body containing
//! the assertion is parsed and type-checked.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::{Checker, Config};
use guff_types_errors::Code;

fn check_src(src: &str) -> Checker {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file]);
    check
}

/// Asserting an empty-interface value to a concrete type is always allowed
/// (the concrete type needs no methods).
#[test]
fn empty_interface_assert_ok() {
    let check = check_src(
        "package p\n\
         func f(i interface{}) {\n_ = i.(int)\n}\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

/// Asserting to a type that provides the interface's method is allowed.
#[test]
fn assert_to_implementing_type_ok() {
    let check = check_src(
        "package p\n\
         type I interface {\nM() int\n}\n\
         type T int\n\
         func (t T) M() int { return 0 }\n\
         func f(i I) {\n_ = i.(T)\n}\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

/// Asserting an interface to a concrete type that cannot possibly implement
/// it is an `ImpossibleAssert` error.
#[test]
fn impossible_assert_errors() {
    let check = check_src(
        "package p\n\
         type I interface {\nM() int\n}\n\
         func f(i I) {\n_ = i.(int)\n}\n",
    );
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == Code::ImpossibleAssert),
        "expected an ImpossibleAssert error, got: {:?}",
        check.errors
    );
}

/// The operand of a type assertion must have interface type.
#[test]
fn assert_on_non_interface_errors() {
    let check = check_src(
        "package p\n\
         func f(n int) {\n_ = n.(int)\n}\n",
    );
    assert!(
        check.errors.iter().any(|e| e.code == Code::InvalidAssert),
        "expected an InvalidAssert error, got: {:?}",
        check.errors
    );
}
