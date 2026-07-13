//! Tests for type-switch statements `switch x.(type) { ... }` (chunk 34b).

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

#[test]
fn basic_type_switch_ok() {
    let check = check_src(
        "package p\n\
         func f(i interface{}) {\n\
         switch i.(type) {\ncase int:\ncase string:\n}\n}\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn type_switch_with_binding_ok() {
    let check = check_src(
        "package p\n\
         func f(i interface{}) {\n\
         switch v := i.(type) {\ncase int:\n_ = v\ncase string:\n_ = v\n}\n}\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn type_switch_nil_case_ok() {
    let check = check_src(
        "package p\n\
         func f(i interface{}) {\n\
         switch i.(type) {\ncase nil:\ncase int:\n}\n}\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn duplicate_case_errors() {
    let check = check_src(
        "package p\n\
         func f(i interface{}) {\n\
         switch i.(type) {\ncase int:\ncase int:\n}\n}\n",
    );
    assert!(
        check.errors.iter().any(|e| e.code == Code::DuplicateCase),
        "expected a DuplicateCase error, got: {:?}",
        check.errors
    );
}

#[test]
fn switch_on_non_interface_errors() {
    let check = check_src(
        "package p\n\
         func f(n int) {\n\
         switch n.(type) {\ncase int:\n}\n}\n",
    );
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == Code::InvalidTypeSwitch),
        "expected an InvalidTypeSwitch error, got: {:?}",
        check.errors
    );
}

#[test]
fn impossible_case_errors() {
    let check = check_src(
        "package p\n\
         type I interface {\nM() int\n}\n\
         func f(i I) {\n\
         switch i.(type) {\ncase int:\n}\n}\n",
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

#[test]
fn type_switch_binding_used_in_one_clause_ok() {
    let check = check_src(
        "package p\n\
         func f(i interface{}) {\n\
         switch v := i.(type) {\ncase int:\n_ = v\ncase string:\n}\n}\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn type_switch_binding_unused_in_all_clauses_errors() {
    let check = check_src(
        "package p\n\
         func f(i interface{}) {\n\
         switch v := i.(type) {\ncase int:\ncase string:\n}\n}\n",
    );
    assert!(
        check.errors.iter().any(|e| e.code == Code::UnusedVar),
        "expected UnusedVar, got: {:?}",
        check.errors
    );
}
