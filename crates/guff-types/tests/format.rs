//! Chunk-59 tests: `format.rs` — the `format.go` message-formatting helpers.
//!
//! Covers the pure helpers (`strip_annotations`, `ndigits`, free `qualifier`)
//! and the `Checker` renderers (`qualifier`, `type_list_str`,
//! `operand_list_str`), plus the qualified `type_str` wiring.

use guff_types::operand::{Operand, OperandMode};
use guff_types::package::new_package;
use guff_types::{ndigits, qualifier, strip_annotations, BasicKind, Checker, Config};

fn int_id(c: &Checker) -> guff_types::TypeId {
    c.typ[BasicKind::Int as usize]
}
fn string_id(c: &Checker) -> guff_types::TypeId {
    c.typ[BasicKind::String as usize]
}

// ---------------------------------------------------------------------------
// strip_annotations

#[test]
fn strip_annotations_removes_subscripts() {
    // Subscript digits ₀..₉ (U+2080..U+2089) are removed.
    assert_eq!(strip_annotations("T₀"), "T");
    assert_eq!(strip_annotations("List[T₁]"), "List[T]");
    assert_eq!(strip_annotations("a₀b₉c"), "abc");
}

#[test]
fn strip_annotations_keeps_hash_and_text() {
    // The guard keeps '#' and ordinary runes untouched.
    assert_eq!(strip_annotations("type#3"), "type#3");
    assert_eq!(strip_annotations("plain string"), "plain string");
    assert_eq!(strip_annotations(""), "");
}

// ---------------------------------------------------------------------------
// ndigits

#[test]
fn ndigits_caps_at_three() {
    assert_eq!(ndigits(0), 1);
    assert_eq!(ndigits(9), 1);
    assert_eq!(ndigits(10), 2);
    assert_eq!(ndigits(99), 2);
    assert_eq!(ndigits(100), 3);
    assert_eq!(ndigits(123456), 3);
}

// ---------------------------------------------------------------------------
// qualifier (free fn + Checker method)

#[test]
fn qualifier_empty_for_current_package() {
    let check = Checker::new(Config::default());
    // The package under check renders unqualified.
    assert_eq!(check.qualifier(check.pkg), "");
    assert_eq!(qualifier(check.pkg, check.pkg, &check.packages), "");
}

#[test]
fn qualifier_names_foreign_package() {
    let mut check = Checker::new(Config::default());
    let foreign = new_package(
        &mut check.packages,
        &mut check.scopes,
        check.universe_scope,
        "math/rand",
        "rand",
    );
    assert_eq!(check.qualifier(foreign), "rand");
    assert_eq!(qualifier(check.pkg, foreign, &check.packages), "rand");
}

// ---------------------------------------------------------------------------
// type_list_str

#[test]
fn type_list_str_bracketed_and_separated() {
    let check = Checker::new(Config::default());
    let int = int_id(&check);
    let s = string_id(&check);
    assert_eq!(check.type_list_str(&[]), "[]");
    assert_eq!(check.type_list_str(&[int]), "[int]");
    assert_eq!(check.type_list_str(&[int, s]), "[int, string]");
}

// ---------------------------------------------------------------------------
// operand_list_str

#[test]
fn operand_list_str_invalid_operands() {
    let check = Checker::new(Config::default());
    assert_eq!(check.operand_list_str(&[]), "[]");
    assert_eq!(
        check.operand_list_str(&[Operand::invalid(), Operand::invalid()]),
        "[invalid operand, invalid operand]"
    );
}

#[test]
fn operand_list_str_value_operand() {
    let check = Checker::new(Config::default());
    let int = int_id(&check);
    let v = Operand {
        mode: OperandMode::Value,
        expr: None,
        typ: Some(int),
        val: None,
        id: None,
    };
    assert_eq!(check.operand_list_str(&[v]), "[value of type int]");
}

// ---------------------------------------------------------------------------
// type_str qualification

#[test]
fn type_str_same_package_is_bare() {
    let check = Checker::new(Config::default());
    let int = int_id(&check);
    // A basic/same-package type renders without a package prefix.
    assert_eq!(check.type_str(int), "int");
}
