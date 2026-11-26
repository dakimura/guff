//! Tests for `Checker::init_order` (port of `initorder.go`).
//!
//! Exercises the package-level variable initialization order derived from the
//! object dependency graph (`add_decl_dep`), plus initialization-cycle
//! detection.

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

/// The initialization order as a list of variable-name groups (one entry per
/// `Initializer`, each entry being the lhs variable names in source order).
fn init_order_names(check: &Checker) -> Vec<Vec<String>> {
    check
        .info
        .init_order
        .iter()
        .map(|init| {
            init.lhs
                .iter()
                .map(|&v| v.name(&check.objects).to_string())
                .collect()
        })
        .collect()
}

/// Flattened single-variable init order (asserts each initializer has exactly
/// one lhs variable).
fn init_order_flat(check: &Checker) -> Vec<String> {
    init_order_names(check)
        .into_iter()
        .map(|mut g| {
            assert_eq!(g.len(), 1, "expected single-variable initializers");
            g.pop().unwrap()
        })
        .collect()
}

#[test]
fn simple_dependency_orders_dependee_first() {
    // x depends on y; y must be initialized first.
    let check = check_src("package p\nvar x = y\nvar y = 1\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(init_order_flat(&check), vec!["y", "x"]);
}

#[test]
fn independent_vars_keep_source_order() {
    let check = check_src("package p\nvar a = 1\nvar b = 2\nvar c = 3\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(init_order_flat(&check), vec!["a", "b", "c"]);
}

#[test]
fn chain_of_dependencies() {
    // a <- b <- c : c first, then b, then a.
    let check = check_src("package p\nvar a = b\nvar b = c\nvar c = 1\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(init_order_flat(&check), vec!["c", "b", "a"]);
}

#[test]
fn dependency_through_function_body() {
    // x = f(), and f's body reads y, so y must be initialized before x.
    let check = check_src(
        "package p\n\
         var x = f()\n\
         func f() int { return y }\n\
         var y = 2\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let order = init_order_flat(&check);
    let y_pos = order
        .iter()
        .position(|n| n == "y")
        .expect("y in init order");
    let x_pos = order
        .iter()
        .position(|n| n == "x")
        .expect("x in init order");
    assert!(y_pos < x_pos, "y should precede x, got {:?}", order);
}

#[test]
fn vars_without_initializers_are_excluded() {
    // `var z int` has no initializer; only x (and its dependency y) appear.
    let check = check_src("package p\nvar z int\nvar x = y\nvar y = 1\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let order = init_order_flat(&check);
    assert!(
        !order.contains(&"z".to_string()),
        "z has no initializer: {:?}",
        order
    );
    assert_eq!(order, vec!["y", "x"]);
}

#[test]
fn constants_do_not_appear_in_init_order() {
    // Constants are dependencies but never emitted as initializers.
    let check = check_src("package p\nconst k = 10\nvar x = k\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(init_order_flat(&check), vec!["x"]);
}

#[test]
fn self_reference_cycle_is_reported() {
    let check = check_src("package p\nvar x = x\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| matches!(e.code, guff_types_errors::Code::InvalidInitCycle)),
        "expected an init-cycle error, got: {:?}",
        check.errors
    );
}

#[test]
fn mutual_cycle_is_reported() {
    let check = check_src("package p\nvar x = y\nvar y = x\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| matches!(e.code, guff_types_errors::Code::InvalidInitCycle)),
        "expected an init-cycle error, got: {:?}",
        check.errors
    );
}
