//! Chunk-57 tests: the in-place `Checker::conversion` driver (D09 recovered).
//! Exercises constant folding (representability rounding + integer→string
//! codepoint), the concise overflow error, and a valid in-range conversion —
//! all end-to-end through a package-level `const` whose RHS is a conversion.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_constant::string_val;
use guff_types::arena::ObjectData;
use guff_types::scope::lookup as scope_lookup;
use guff_types::{Checker, Config};
use guff_types_errors::Code;

fn check_src(src: &str) -> Checker {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file]);
    check
}

fn const_string(check: &Checker, name: &str) -> String {
    let pkg_scope = check.packages.get(check.pkg).scope();
    let obj = scope_lookup(&check.scopes, pkg_scope, name).expect("const not found");
    match check.objects.get(obj) {
        ObjectData::Const(c) => string_val(c.val()),
        _ => panic!("{name} is not a constant"),
    }
}

fn has_invalid_conversion(check: &Checker) -> bool {
    check
        .errors
        .iter()
        .any(|e| e.code == Code::InvalidConversion)
}

#[test]
fn integer_to_string_folds_codepoint() {
    // string(65) is the one-rune string "A".
    let c = check_src("package p\nconst s = string(65)\n");
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
    assert_eq!(const_string(&c, "s"), "A");
}

#[test]
fn constant_conversion_overflow_errors() {
    // byte(256) overflows uint8.
    let c = check_src("package p\nconst b = byte(256)\n");
    assert!(
        has_invalid_conversion(&c),
        "expected overflow error, got: {:?}",
        c.errors
    );
}

#[test]
fn int8_in_range_is_ok() {
    let c = check_src("package p\nconst a = int8(127)\n");
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn int8_overflow_errors() {
    let c = check_src("package p\nconst a = int8(200)\n");
    assert!(
        has_invalid_conversion(&c),
        "expected overflow error, got: {:?}",
        c.errors
    );
}

#[test]
fn float_to_int_truncation_ok() {
    // float constant with an integer value converts cleanly.
    let c = check_src("package p\nconst n = int(3.0)\n");
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn float_to_int_with_fraction_errors() {
    // 3.5 is not representable as an int.
    let c = check_src("package p\nconst n = int(3.5)\n");
    assert!(
        has_invalid_conversion(&c),
        "expected conversion error, got: {:?}",
        c.errors
    );
}
