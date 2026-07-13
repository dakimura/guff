//! Tests for n:1 variable declarations (chunk 78): `var a, b = f()` where a
//! single multi-valued expression initializes several variables, both at
//! package level and inside function bodies. Mirrors Go's varDecl/initVars
//! `lhs` handling.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::scope::lookup as scope_lookup;
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

/// The rendered type of a package-level object.
fn type_of(check: &Checker, name: &str) -> String {
    let pkg_scope = check.packages.get(check.pkg).scope();
    let obj = scope_lookup(&check.scopes, pkg_scope, name).unwrap_or_else(|| panic!("no {name}"));
    let t = obj.typ(&check.objects).unwrap_or_else(|| panic!("{name} has no type"));
    check.type_str(t)
}

#[test]
fn package_level_n_to_1_spreads_tuple_types() {
    let check = check_src(
        "package p\n\
         func f() (int, string) { return 0, \"\" }\n\
         var a, b = f()\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(type_of(&check, "a"), "int");
    assert_eq!(type_of(&check, "b"), "string");
}

#[test]
fn package_level_n_to_1_with_declared_type() {
    // `var a, b int = g()` — the declared type applies to both variables.
    let check = check_src(
        "package p\n\
         func g() (int, int) { return 1, 2 }\n\
         var a, b int = g()\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(type_of(&check, "a"), "int");
    assert_eq!(type_of(&check, "b"), "int");
}

#[test]
fn package_level_n_to_1_comma_ok_map_index() {
    // `var v, ok = m[k]` — comma-ok spread yields (elem, bool).
    let check = check_src(
        "package p\n\
         var m = map[string]int{}\n\
         var v, ok = m[\"k\"]\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(type_of(&check, "v"), "int");
    assert_eq!(type_of(&check, "ok"), "bool");
}

#[test]
fn package_level_n_to_1_count_mismatch_is_error() {
    // f returns 2 values but 3 variables are declared.
    let check = check_src(
        "package p\n\
         func f() (int, string) { return 0, \"\" }\n\
         var a, b, c = f()\n",
    );
    assert!(
        !check.errors.is_empty(),
        "a count mismatch should be reported"
    );
}

#[test]
fn local_n_to_1_spreads_tuple_types() {
    // Inside a function body, `var a, b = f()` must also work.
    let check = check_src(
        "package p\n\
         func f() (int, string) { return 0, \"\" }\n\
         func use() { var a, b = f(); _ = a; _ = b }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}
