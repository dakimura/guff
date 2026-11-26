//! Go 1.27: promoted fields as struct-literal keys, and inferring a generic
//! function's type arguments in every assignment context. Shapes from go1.27's
//! `internal/types/testdata/spec/structLits.go` and
//! `fixedbugs/issue77245.go` / `issue80042.go`.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_types::{Checker, Config};
use guff_types_errors::Code;

fn check_versioned(src: &str, version: &str) -> Checker {
    let fset = FileSet::new();
    let mut file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
    file.go_version = version.to_string();
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file]);
    check
}

fn messages(check: &Checker) -> Vec<String> {
    check.errors.iter().map(|e| e.msg.clone()).collect()
}

const PROMOTED: &str = r#"package p

type Bar struct{ Baz int }

type Foo struct {
	Bar
	Qux string
}

var _ = Foo{Baz: 1, Qux: "q"}
"#;

#[test]
fn promoted_field_key_checks_at_go127() {
    let c = check_versioned(PROMOTED, "go1.27");
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", messages(&c));
}

#[test]
fn promoted_field_key_requires_go127() {
    let c = check_versioned(PROMOTED, "go1.26");
    let msgs = messages(&c);
    assert!(
        c.errors.iter().any(|e| e.code == Code::UnsupportedFeature
            && e.msg.contains("use of promoted field Bar.Baz in struct literal of type Foo")),
        "errors: {msgs:?}"
    );
}

#[test]
fn promoted_field_key_overlaps() {
    let src = r#"package p

type B struct{ b int }

type C struct {
	B
	x int
}

var _ = C{B: B{}, b: 1}
var _ = C{b: 1, B: B{}}
var _ = C{x: 1, x: 2}
"#;
    let msgs = messages(&check_versioned(src, "go1.27"));
    for want in [
        "cannot specify promoted field b and enclosing embedded field B",
        "cannot specify embedded field B and enclosed promoted field b",
        "duplicate field name x in struct literal",
    ] {
        assert!(msgs.iter().any(|m| m == want), "missing {want:?} in {msgs:?}");
    }
}

#[test]
fn promoted_field_key_through_a_pointer_is_rejected() {
    let src = r#"package p

type B struct{ b int }

type C struct{ *B }

var _ = C{b: 1}
"#;
    let msgs = messages(&check_versioned(src, "go1.27"));
    assert!(
        msgs.iter().any(|m| m == "invalid implicit pointer indirection to reach b"),
        "errors: {msgs:?}"
    );
}

#[test]
fn generic_function_inferred_in_every_assignment_context() {
    let src = r#"package p

func g[P any](P) {}

type F func(int)

type S struct{ f F }

func use[Q func(string)](ch chan F) {
	_ = []F{g}
	_ = S{f: g}
	_ = map[string]F{"a": g}
	_ = map[*F]bool{}
	ch <- g
	_ = F(g)
	var q Q = g
	_ = q
}
"#;
    let c = check_versioned(src, "go1.27");
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", messages(&c));
}
