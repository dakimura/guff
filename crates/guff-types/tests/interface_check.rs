//! Tests for interface type-expression checking (`interface_check.rs`, 33b).

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::{Checker, Config};

fn check_src(src: &str) -> Checker {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file]);
    check
}

#[test]
fn empty_interface_accepts_any_value() {
    let check = check_src("package p\nvar x interface{} = 5\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn interface_type_declaration_resolves() {
    let check = check_src("package p\ntype Stringer interface {\nString() string\n}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn interface_satisfied_by_method_holder() {
    let check = check_src(
        "package p\n\
         type I interface {\nM() int\n}\n\
         type T int\n\
         func (t T) M() int { return 0 }\n\
         var i I = T(0)\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn interface_not_satisfied_errors() {
    let check = check_src(
        "package p\n\
         type I interface {\nM() int\n}\n\
         type T int\n\
         var i I = T(0)\n",
    );
    assert!(
        !check.errors.is_empty(),
        "expected an interface-satisfaction error"
    );
}

#[test]
fn embedded_interface_resolves() {
    let check = check_src(
        "package p\n\
         type I interface {\nM() int\n}\n\
         type J interface {\nI\nN() int\n}\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn type_constraint_union_resolves() {
    // A constraint-style interface with a `~int | ~string` union element.
    let check = check_src("package p\ntype C interface {\n~int | ~string\n}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn blank_interface_method_errors() {
    let check = check_src("package p\ntype I interface {\n_() int\n}\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::BlankIfaceMethod),
        "expected BlankIfaceMethod, got: {:?}",
        check.errors
    );
}
