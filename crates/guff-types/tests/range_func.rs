//! Tests for range-over-func (chunk 79, go1.23): `for k, v := range f` where
//! `f` is `func(yield func(K[, V]) bool)`. The key/value types come from the
//! yield callback's parameters. Mirrors Go's rangeKeyVal `*Signature` case.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::{Checker, Config};

fn parse(src: &str) -> guff::ast::File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

fn check_src(src: &str) -> Checker {
    let mut check = Checker::new(Config::default());
    check.check_files(vec![parse(src)]);
    check
}

#[test]
fn single_value_iterator_binds_key_type() {
    // `for k := range Seq` — k has the yield param type (int).
    let check = check_src(
        "package p\n\
         func Seq(yield func(int) bool) {}\n\
         func use() { for k := range Seq { var n int = k; _ = n } }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn key_value_iterator_binds_both_types() {
    // `for k, v := range Seq2` — k int, v string.
    let check = check_src(
        "package p\n\
         func Seq2(yield func(int, string) bool) {}\n\
         func use() {\n\
             for k, v := range Seq2 { var a int = k; var b string = v; _ = a; _ = b }\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn iterator_key_type_mismatch_is_error() {
    // k is int, so assigning it to a string must fail.
    let check = check_src(
        "package p\n\
         func Seq(yield func(int) bool) {}\n\
         func use() { for k := range Seq { var s string = k; _ = s } }\n",
    );
    assert!(
        !check.errors.is_empty(),
        "assigning int key to string should error"
    );
}

#[test]
fn non_iterator_func_cannot_be_ranged() {
    // `func() int` is not a valid iterator (wrong param/result shape).
    let check = check_src(
        "package p\n\
         func Bad() int { return 0 }\n\
         func use() { for range Bad {} }\n",
    );
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.msg.contains("cannot range over")),
        "a non-iterator func must be rejected; got: {:?}",
        check.errors
    );
}

#[test]
fn yield_must_return_plain_bool() {
    // A yield func returning a named boolean type is rejected (issue 71131).
    let check = check_src(
        "package p\n\
         type MyBool bool\n\
         func Seq(yield func(int) MyBool) {}\n\
         func use() { for range Seq {} }\n",
    );
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.msg.contains("cannot range over")),
        "yield returning a named bool must be rejected; got: {:?}",
        check.errors
    );
}
