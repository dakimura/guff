//! Tests for return-value count checking (chunk 80): a `return` whose value
//! count does not match the function's results is reported with
//! `WrongResultCount`. Mirrors Go's returnError (assignments.go).

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

fn result_count_errors(check: &Checker) -> Vec<&str> {
    check
        .errors
        .iter()
        .filter(|e| e.code == Code::WrongResultCount)
        .map(|e| e.msg.as_str())
        .collect()
}

#[test]
fn too_many_return_values_is_reported() {
    let check = check_src(
        "package p\n\
         func f() int { return 1, 2 }\n",
    );
    assert_eq!(
        result_count_errors(&check),
        vec!["too many return values"],
        "all errors: {:?}",
        check.errors
    );
}

#[test]
fn not_enough_return_values_is_reported() {
    let check = check_src(
        "package p\n\
         func f() (int, int) { return 1 }\n",
    );
    assert_eq!(
        result_count_errors(&check),
        vec!["not enough return values"],
        "all errors: {:?}",
        check.errors
    );
}

#[test]
fn bare_return_with_unnamed_results_is_reported() {
    // `return` with no values in a function with (unnamed) results is an error.
    let check = check_src(
        "package p\n\
         func f() int { return }\n",
    );
    assert_eq!(
        result_count_errors(&check),
        vec!["not enough return values"],
        "all errors: {:?}",
        check.errors
    );
}

#[test]
fn matching_return_count_is_ok() {
    let check = check_src(
        "package p\n\
         func f() (int, string) { return 1, \"x\" }\n",
    );
    assert!(
        result_count_errors(&check).is_empty(),
        "a matching return must not error: {:?}",
        check.errors
    );
}

#[test]
fn bare_return_with_named_results_is_ok() {
    // Named results allow an empty `return`.
    let check = check_src(
        "package p\n\
         func f() (a int, b string) { return }\n",
    );
    assert!(
        result_count_errors(&check).is_empty(),
        "bare return with named results must be allowed: {:?}",
        check.errors
    );
}

#[test]
fn return_from_multi_value_call_is_ok() {
    // `return g()` where g returns exactly the function's results.
    let check = check_src(
        "package p\n\
         func g() (int, string) { return 1, \"x\" }\n\
         func f() (int, string) { return g() }\n",
    );
    assert!(
        result_count_errors(&check).is_empty(),
        "spreading a matching call must not error: {:?}",
        check.errors
    );
}

fn out_of_scope_errors(check: &Checker) -> Vec<&str> {
    check
        .errors
        .iter()
        .filter(|e| e.code == Code::OutOfScopeResult)
        .map(|e| e.msg.as_str())
        .collect()
}

#[test]
fn bare_return_with_shadowed_named_result_is_reported() {
    // The inner `n` shadows the named result `n`, so a bare `return` there is
    // disallowed (go spec implementation restriction).
    let check = check_src(
        "package p\n\
         func f() (n int) {\n\
             {\n\
                 n := 5\n\
                 _ = n\n\
                 return\n\
             }\n\
         }\n",
    );
    assert_eq!(
        out_of_scope_errors(&check),
        vec!["result parameter n not in scope at return"],
        "all errors: {:?}",
        check.errors
    );
}

#[test]
fn bare_return_without_shadowing_is_ok() {
    let check = check_src(
        "package p\n\
         func f() (n int) {\n\
             n = 5\n\
             return\n\
         }\n",
    );
    assert!(
        out_of_scope_errors(&check).is_empty(),
        "unshadowed named result must be fine: {:?}",
        check.errors
    );
}
