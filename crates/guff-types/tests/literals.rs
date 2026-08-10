//! Tests for composite/function literal checking (`literals.rs`, chunk 31).
//!
//! Composite literals are exercised through `check_files` (package-level `var`
//! initializers). Struct literals need `typexpr`'s struct-type support (still
//! deferred), so only slice/array/map literals are covered here; the struct
//! path (`composite_struct`) is implemented but not yet reachable via the
//! driver.

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
fn slice_literal_checks_cleanly() {
    let check = check_src("package p\nvar v = []int{1, 2, 3}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn array_literal_checks_cleanly() {
    let check = check_src("package p\nvar v = [3]int{1, 2, 3}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn open_array_literal_checks_cleanly() {
    let check = check_src("package p\nvar v = [...]int{1, 2, 3}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn map_literal_checks_cleanly() {
    let check = check_src("package p\nvar v = map[string]int{\"a\": 1, \"b\": 2}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn map_literal_duplicate_string_key_errors() {
    let check = check_src("package p\nvar v = map[string]int{\"a\": 1, \"a\": 2}\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::DuplicateLitKey),
        "expected DuplicateLitKey, got: {:?}",
        check.errors
    );
}

#[test]
fn map_literal_byte_escape_and_code_point_are_distinct_keys() {
    // A Go string is bytes: "\xff" is the single byte 0xFF and "\u00ff" is the
    // two bytes of U+00FF. Decoding the byte escape into a code point made
    // them equal, so this well-formed package was reported as having a
    // duplicate key — and an ill-typed package is one guff then skips whole.
    let check = check_src("package p\nvar v = map[string]int{\"\\xff\": 1, \"\\u00ff\": 2}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn map_literal_duplicate_int_key_errors() {
    let check = check_src("package p\nvar v = map[int]string{1: \"a\", 1: \"b\"}\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::DuplicateLitKey),
        "expected DuplicateLitKey, got: {:?}",
        check.errors
    );
}

#[test]
fn map_literal_int_float_keys_collide() {
    // 1 and 1.0 normalise to the same key (concrete key type int via 1.0).
    let check = check_src("package p\nvar v = map[float64]string{1: \"a\", 1.0: \"b\"}\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::DuplicateLitKey),
        "expected DuplicateLitKey for 1 vs 1.0, got: {:?}",
        check.errors
    );
}

#[test]
fn map_literal_distinct_keys_ok() {
    let check = check_src("package p\nvar v = map[int]int{1: 1, 2: 2, 3: 3}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn map_literal_non_constant_keys_not_deduped() {
    // Variable keys are never duplicate-checked.
    let check = check_src("package p\nfunc f(i, j int) { _ = map[int]int{i: 1, j: 2, i: 3} }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn keyed_slice_literal_checks_cleanly() {
    let check = check_src("package p\nvar v = []int{0: 1, 2: 3}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn oversize_array_literal_errors() {
    let check = check_src("package p\nvar v = [2]int{1, 2, 3}\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::OversizeArrayLit),
        "expected OversizeArrayLit, got: {:?}",
        check.errors
    );
}

#[test]
fn duplicate_index_in_slice_literal_errors() {
    let check = check_src("package p\nvar v = []int{0: 1, 0: 2}\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::DuplicateLitKey),
        "expected DuplicateLitKey, got: {:?}",
        check.errors
    );
}

#[test]
fn missing_key_in_map_literal_errors() {
    let check = check_src("package p\nvar v = map[string]int{1}\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::MissingLitKey),
        "expected MissingLitKey, got: {:?}",
        check.errors
    );
}

#[test]
fn wrong_element_type_in_slice_literal_errors() {
    let check = check_src("package p\nvar v = []int{\"x\"}\n");
    assert!(!check.errors.is_empty(), "expected an element type error");
}

#[test]
fn struct_literal_positional_checks_cleanly() {
    let check = check_src("package p\ntype T struct {\na int\nb int\n}\nvar v = T{1, 2}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn struct_literal_keyed_checks_cleanly() {
    let check = check_src("package p\ntype T struct {\na int\nb int\n}\nvar v = T{a: 1, b: 2}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn struct_literal_unknown_field_errors() {
    let check = check_src("package p\ntype T struct {\na int\n}\nvar v = T{c: 1}\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::MissingLitField),
        "expected MissingLitField, got: {:?}",
        check.errors
    );
}

#[test]
fn struct_literal_too_many_values_errors() {
    let check = check_src("package p\ntype T struct {\na int\n}\nvar v = T{1, 2}\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::InvalidStructLit),
        "expected InvalidStructLit, got: {:?}",
        check.errors
    );
}

#[test]
fn duplicate_struct_field_name_errors() {
    let check = check_src("package p\ntype T struct {\na int\na int\n}\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::DuplicateDecl),
        "expected DuplicateDecl, got: {:?}",
        check.errors
    );
}

#[test]
fn struct_field_access_checks_cleanly() {
    let check = check_src("package p\ntype T struct {\na int\n}\nfunc f(t T) int { return t.a }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn nested_literal_with_explicit_inner_type_checks_cleanly() {
    let check = check_src("package p\nvar v = [][]int{[]int{1, 2}, []int{3}}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn typeless_nested_slice_elements_check_cleanly() {
    // exprWithHint (chunk 86): the inner `{...}` picks up the `[]int` element
    // type of the outer `[][]int` literal.
    let check = check_src("package p\nvar v = [][]int{{1, 2}, {3}}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn typeless_nested_struct_slice_elements_check_cleanly() {
    // `[]Point{{1, 2}, {3, 4}}` — each typeless `{...}` resolves to the slice
    // element type `Point`.
    let check = check_src(
        "package p\ntype Point struct {\nx int\ny int\n}\nvar v = []Point{{1, 2}, {3, 4}}\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn typeless_array_elements_check_cleanly() {
    let check = check_src("package p\nvar v = [2][]int{{1}, {2, 3}}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn typeless_map_value_checks_cleanly() {
    // Map values get the element-type hint.
    let check = check_src("package p\nvar v = map[string][]int{\"a\": {1, 2}, \"b\": {3}}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn typeless_map_key_checks_cleanly() {
    // Map keys get the key-type hint (`[2]int` here).
    let check = check_src("package p\nvar v = map[[2]int]bool{{1, 2}: true}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn typeless_nested_struct_field_still_errors() {
    // Go does NOT propagate the hint into struct fields: a nested `{...}` in a
    // keyed struct literal still needs an explicit type.
    let check = check_src(
        "package p\ntype Inner struct {\na int\n}\ntype Outer struct {\ni Inner\n}\nvar v = Outer{i: {1}}\n",
    );
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::UntypedLit),
        "expected UntypedLit for typeless struct field, got: {:?}",
        check.errors
    );
}
