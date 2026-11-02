//! Go 1.27 generic methods: a method may declare its own type parameters
//! (`func (T) M[P any]()`). Shapes from go1.27's
//! `internal/types/testdata/spec/methods.go`.

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

const OK_SRC: &str = r#"package p

type T struct{}

func (T) Map[U any](u U) U { return u }

func (t *T) Pair[A, B any](a A, b B) (A, B) { return a, b }

type G[F any] struct{ v F }

func (g G[F]) With[P any](p P) (F, P) { return g.v, p }

func (g G[F]) Deref[P interface{ ~*F }](p P) F { return *p }

func use() {
	var t T
	_ = t.Map[int](1)
	_ = t.Map("s")
	f := t.Map[string]
	_ = f("x")
	g := T.Map[int]
	_ = g(t, 1)
	var h func(T, int) int = T.Map
	_ = h
	a, b := (&t).Pair(1, "x")
	_, _ = a, b
	gi := G[int]{v: 1}
	x, y := gi.With("q")
	_, _ = x, y
	i := 3
	_ = gi.Deref(&i)
}
"#;

#[test]
fn generic_methods_check_at_go127() {
    let c = check_versioned(OK_SRC, "go1.27");
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", messages(&c));
}

#[test]
fn generic_method_requires_go127() {
    let c = check_versioned(OK_SRC, "go1.26");
    let unsupported: Vec<String> = c
        .errors
        .iter()
        .filter(|e| e.code == Code::UnsupportedFeature)
        .map(|e| e.msg.clone())
        .collect();
    assert_eq!(unsupported.len(), 4, "errors: {:?}", messages(&c));
    assert!(
        unsupported.iter().all(|m| m.contains("generic method")),
        "errors: {:?}",
        unsupported
    );
}

/// The method's bound refers to the receiver's type parameter, so it must be
/// substituted per instance: `G[int].Deref` takes `~*int`, not `~*F`.
///
/// go1.27 says `P (type *string) does not satisfy interface{~*int}`; guff says
/// `cannot infer type arguments in call`, as it already does for the plain
/// function `D[int](&s)` — an older difference in inference's error, not
/// this one.
#[test]
fn method_bound_follows_the_receiver_instance() {
    let src = |arg: &str| {
        format!(
            "package p\n\ntype G[F any] struct{{}}\n\nfunc (G[F]) Deref[P interface{{ ~*F }}](p P) {{}}\n\nfunc use() {{\n\ts := {arg}\n\tG[int]{{}}.Deref(&s)\n}}\n"
        )
    };
    let c = check_versioned(&src("1"), "go1.27");
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", messages(&c));
    let c = check_versioned(&src("\"\""), "go1.27");
    assert!(!c.errors.is_empty(), "expected *string to be rejected");
}

/// A generic method never implements an interface method.
#[test]
fn generic_method_does_not_satisfy_an_interface() {
    let src = r#"package p

type I interface{ M(int) }

type T struct{}

func (T) M[P any](P) {}

var _ I = T{}
"#;
    let c = check_versioned(src, "go1.27");
    assert!(!c.errors.is_empty(), "expected an assignability error");
}

