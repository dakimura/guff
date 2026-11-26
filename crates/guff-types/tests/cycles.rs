//! Tests for `direct_cycles` (chunk 60, `cycles.go` port) — detection of direct
//! name-chain cycles among package-level type declarations.
//!
//! These cycles (`type A B; type B A`, `type A = B; type B = A`, `type A A`)
//! are not caught by `valid_type` (which only inspects a defined type's
//! underlying structure), so without `direct_cycles` they would be silently
//! invalidated with no diagnostic.

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

fn has_cycle_error(check: &Checker) -> bool {
    check
        .errors
        .iter()
        .any(|e| e.code == Code::InvalidDeclCycle)
}

fn count_cycle_errors(check: &Checker) -> usize {
    check
        .errors
        .iter()
        .filter(|e| e.code == Code::InvalidDeclCycle)
        .count()
}

#[test]
fn mutual_defined_type_cycle() {
    // type A B; type B A — a direct cycle via regular declarations.
    let check = check_src("package p\ntype A B\ntype B A\n");
    assert!(
        has_cycle_error(&check),
        "expected a cycle error: {:?}",
        check.errors
    );
}

#[test]
fn mutual_alias_cycle() {
    // type A = B; type B = A — a direct cycle via aliases.
    let check = check_src("package p\ntype A = B\ntype B = A\n");
    assert!(
        has_cycle_error(&check),
        "expected a cycle error: {:?}",
        check.errors
    );
}

#[test]
fn self_referential_type() {
    // type A A — self reference.
    let check = check_src("package p\ntype A A\n");
    assert!(
        has_cycle_error(&check),
        "expected a cycle error: {:?}",
        check.errors
    );
    let msg = &check
        .errors
        .iter()
        .find(|e| e.code == Code::InvalidDeclCycle)
        .unwrap()
        .msg;
    assert!(msg.contains("refers to itself"), "message was: {msg}");
}

#[test]
fn three_way_cycle() {
    // type A B; type B C; type C A.
    let check = check_src("package p\ntype A B\ntype B C\ntype C A\n");
    assert!(
        has_cycle_error(&check),
        "expected a cycle error: {:?}",
        check.errors
    );
    // Only one cycle is reported (the rest of the chain is marked black).
    assert_eq!(count_cycle_errors(&check), 1, "errors: {:?}", check.errors);
}

#[test]
fn no_cycle_chain_ending_in_basic() {
    // type A B; type B int — B resolves to a universe type, not a pkg type.
    let check = check_src("package p\ntype A B\ntype B int\n");
    assert!(
        !has_cycle_error(&check),
        "unexpected cycle error: {:?}",
        check.errors
    );
}

#[test]
fn no_cycle_simple_alias_chain() {
    // type A int; type B A — terminates at int, no cycle.
    let check = check_src("package p\ntype A int\ntype B A\n");
    assert!(
        !has_cycle_error(&check),
        "unexpected cycle error: {:?}",
        check.errors
    );
}

#[test]
fn no_cycle_through_type_literal() {
    // type A *B; type B A — A's RHS is a pointer literal, not a bare name, so
    // direct_cycle stops at A (this finite indirection cycle is fine).
    let check = check_src("package p\ntype A *B\ntype B A\n");
    assert!(
        !has_cycle_error(&check),
        "unexpected cycle error: {:?}",
        check.errors
    );
}
