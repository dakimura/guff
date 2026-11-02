//! A type parameter is comparable when its constraint's type set is: for a
//! non-interface bound (`[P T]`) that is the implicit `interface{ T }`.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_types::{Checker, Config};

fn check_src(src: &str) -> Checker {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file]);
    check
}

/// guff took the bound's own underlying type (`int`'s named `T`, not an
/// interface) and called `P` incomparable, so `switch p {}` made the whole
/// package ill-typed and every finding in it disappeared.
#[test]
fn switch_on_a_type_parameter_with_a_non_interface_bound() {
    let src = r#"package p

type T int

func g[P T](p P) {
	switch p {
	}
	_ = p == p
}
"#;
    let c = check_src(src);
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

/// Measured with go1.26.5: `switch p {}` on a slice-bound `P` is allowed
/// (a slice has nil), but `p == p` is "incomparable types in type set". guff
/// rejects the same one, worded `operator EQL not defined on operands` — an
/// older difference; type errors are not findings.
#[test]
fn a_type_parameter_with_an_incomparable_bound_is_still_rejected() {
    let src = r#"package p

type S []int

func g[P S](p P) {
	switch p {
	}
}

func h[P S](p P) {
	_ = p == p
}
"#;
    let c = check_src(src);
    assert_eq!(c.errors.len(), 1, "errors: {:?}", c.errors);
}
