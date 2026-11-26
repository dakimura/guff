//! Tests for `mono.rs` (chunk 58) — detection of unbounded recursive
//! instantiation (non-monomorphizable packages).
//!
//! The graph-detection core (`MonoGraph::record_instance` + `monomorph`) is
//! exercised directly: we build type parameters in the checker's arenas and
//! feed instantiations to the graph, then assert whether a positive-weight
//! cycle is reported. Driving this fully through source (`F[*T]()` inside a
//! generic body) is blocked for now: explicit `f[T]()` call-position
//! instantiation is deferred (D21), and inference rejects a recursively
//! parameterized result like `T := *T` (D11) before the graph ever sees it.
//! The whole-package smoke tests at the bottom confirm the pass is a no-op
//! for ordinary code.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::{new_pointer, new_type_name, new_type_param, Checker, Config, TypeId};
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
        .any(|e| e.code == Code::InvalidInstanceCycle)
}

/// Creates a fresh type parameter named `name` belonging to the checker's
/// package (so `assign`'s same-package guard admits it). Returns the
/// `TypeParam` `TypeId`.
fn new_local_tparam(check: &mut Checker, name: &str) -> TypeId {
    let tn = new_type_name(&mut check.objects, name, None);
    let pkg = check.pkg;
    tn.set_pkg(&mut check.objects, pkg);
    new_type_param(&mut check.types, tn, None)
}

/// `T` instantiated as `*T` is a derived type: the self-edge `T <- T` has
/// weight 1, which is a positive-weight cycle (unbounded instantiation).
#[test]
fn pointer_self_instantiation_is_a_cycle() {
    let mut check = Checker::new(Config::default());
    let t = new_local_tparam(&mut check, "T");
    let ptr_t = new_pointer(&mut check.types, t);

    check.mono.record_instance(
        &check.types,
        &check.objects,
        &check.scopes,
        &check.packages,
        check.pkg,
        0,
        &[t],
        &[ptr_t],
        &[],
    );

    check.monomorph();
    assert!(
        has_cycle_error(&check),
        "expected a positive-weight cycle, got: {:?}",
        check.errors
    );
}

/// `T` instantiated as `T` itself is a zero-weight self-edge: allowed,
/// because static instantiation reaches a fixed point.
#[test]
fn identity_self_instantiation_is_not_a_cycle() {
    let mut check = Checker::new(Config::default());
    let t = new_local_tparam(&mut check, "T");

    check.mono.record_instance(
        &check.types,
        &check.objects,
        &check.scopes,
        &check.packages,
        check.pkg,
        0,
        &[t],
        &[t],
        &[],
    );

    check.monomorph();
    assert!(
        !has_cycle_error(&check),
        "zero-weight self-edge should be allowed, got: {:?}",
        check.errors
    );
}

/// `T` instantiated with a concrete `int` produces no flow edges at all.
#[test]
fn concrete_instantiation_has_no_edges() {
    let mut check = Checker::new(Config::default());
    let t = new_local_tparam(&mut check, "T");
    let int_t = check.typ[guff_types::BasicKind::Int as usize];

    check.mono.record_instance(
        &check.types,
        &check.objects,
        &check.scopes,
        &check.packages,
        check.pkg,
        0,
        &[t],
        &[int_t],
        &[],
    );

    check.monomorph();
    assert!(
        !has_cycle_error(&check),
        "concrete instantiation should not cycle, got: {:?}",
        check.errors
    );
}

/// A two-parameter swap (`A := B`, `B := A`) forms only zero-weight edges
/// (each target *is* the other parameter): an allowed fixed-point cycle.
#[test]
fn two_param_swap_is_zero_weight() {
    let mut check = Checker::new(Config::default());
    let a = new_local_tparam(&mut check, "A");
    let b = new_local_tparam(&mut check, "B");

    check.mono.record_instance(
        &check.types,
        &check.objects,
        &check.scopes,
        &check.packages,
        check.pkg,
        0,
        &[a, b],
        &[b, a],
        &[],
    );

    check.monomorph();
    assert!(
        !has_cycle_error(&check),
        "parameter swap is zero-weight, got: {:?}",
        check.errors
    );
}

/// `A` instantiated as `map[A]A` derives from `A` twice (weight-1 edges):
/// a positive-weight self-cycle.
#[test]
fn map_self_instantiation_is_a_cycle() {
    use guff_types::new_map;

    let mut check = Checker::new(Config::default());
    let a = new_local_tparam(&mut check, "A");
    let map_aa = new_map(&mut check.types, a, a);

    check.mono.record_instance(
        &check.types,
        &check.objects,
        &check.scopes,
        &check.packages,
        check.pkg,
        0,
        &[a],
        &[map_aa],
        &[],
    );

    check.monomorph();
    assert!(
        has_cycle_error(&check),
        "map[A]A derives A: expected a cycle, got: {:?}",
        check.errors
    );
}

/// Imported (other-package) type parameters are ignored: `assign` bails when
/// the type parameter's object does not belong to the package being checked.
#[test]
fn foreign_package_tparam_is_ignored() {
    let mut check = Checker::new(Config::default());
    // A type parameter whose object has no package (not `check.pkg`).
    let tn = new_type_name(&mut check.objects, "T", None);
    let t = new_type_param(&mut check.types, tn, None);
    let ptr_t = new_pointer(&mut check.types, t);

    check.mono.record_instance(
        &check.types,
        &check.objects,
        &check.scopes,
        &check.packages,
        check.pkg,
        0,
        &[t],
        &[ptr_t],
        &[],
    );

    check.monomorph();
    assert!(
        !has_cycle_error(&check),
        "foreign type parameter must be ignored, got: {:?}",
        check.errors
    );
}

// ---------------------------------------------------------------------------
// Whole-package smoke tests: `monomorph` must be a no-op for ordinary code.
// ---------------------------------------------------------------------------

#[test]
fn concrete_generic_call_is_not_a_cycle() {
    let check = check_src(
        "package p\n\
         func id[T any](x T) T { return x }\n\
         func g() { id(0) }\n",
    );
    assert!(
        !has_cycle_error(&check),
        "concrete generic call should not cycle, got: {:?}",
        check.errors
    );
}

#[test]
fn non_generic_package_has_no_cycle() {
    let check = check_src(
        "package p\n\
         type T int\n\
         func f(a int) int { return a }\n",
    );
    assert!(
        !has_cycle_error(&check),
        "non-generic package should not report a cycle: {:?}",
        check.errors
    );
}
