//! `typeparams::Free` — does this type mention a type parameter that is still
//! free?
//!
//! Port of `golang.org/x/tools/internal/typeparams/free.go`, which
//! `ifaceassert` consults before drawing any conclusion about a type
//! assertion. The arms are one-liners, so the risk is not that one is subtly
//! wrong — it is that one is never reached and quietly answers `false` for a
//! type full of type parameters. Every shape below is one arm, paired with the
//! same shape written over `int`, which must answer `false`.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::scope::lookup as scope_lookup;
use guff_types::signature::signature_params;
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::typeparams::{has_free_type_param, Free};
use guff_types::{Checker, Config};

const SRC: &str = "\
package p

type Box[T any] struct{ V T }

type Pair[A any, B any] interface {
	Get() A
	Put(B) error
}

type Num interface{ ~int | ~float64 }

type Alias[T any] = []T

func freeSlice[T any](x []T)                             {}
func freeArray[T any](x [4]T)                            {}
func freePointer[T any](x *T)                            {}
func freeChan[T any](x chan T)                           {}
func freeMapKey[T any](x map[T]int)                      {}
func freeMapElem[T any](x map[string]T)                  {}
func freeStruct[T any](x struct{ F T })                  {}
func freeSignature[T any](x func(int) (T, error))        {}
func freeNamed[T any](x Box[T])                          {}
func freeNamedIface[T any](x Pair[T, int])               {}
func freeAnonIface[T any](x interface{ Get() []T })      {}
func freeNested[T any](x []map[string][]*Box[T])         {}
func freeParam[T any](x T)                               {}
func freeAlias[T any](x Alias[T])                        {}

func fixedSlice(x []int)                                 {}
func fixedArray(x [4]int)                                {}
func fixedPointer(x *int)                                {}
func fixedChan(x chan int)                               {}
func fixedMapKey(x map[int]int)                          {}
func fixedMapElem(x map[string]int)                      {}
func fixedStruct(x struct{ F int })                      {}
func fixedSignature(x func(int) (int, error))            {}
func fixedNamed(x Box[int])                              {}
func fixedNamedIface(x Pair[int, string])                {}
func fixedAnonIface(x interface{ Get() []int })          {}
func fixedNested(x []map[string][]*Box[int])             {}
func fixedBasic(x int)                                   {}
func fixedAlias(x Alias[int])                            {}
func fixedConstraint(x Num)                              {}
";

/// The type of the single parameter of package-level function `name`.
fn param_type(check: &Checker, name: &str) -> guff_types::TypeId {
    let pkg_scope = check.packages.get(check.pkg).scope();
    let f = scope_lookup(&check.scopes, pkg_scope, name)
        .unwrap_or_else(|| panic!("no function {name}"));
    let sig = f.typ(&check.objects).expect("function has a type");
    let params = signature_params(&check.types, sig);
    assert_eq!(
        tuple_len(&check.types, params),
        1,
        "{name} should take exactly one parameter"
    );
    tuple_at(&check.types, params.expect("params"), 0)
        .typ(&check.objects)
        .expect("parameter has a type")
}

fn checked() -> Checker {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", SRC.as_bytes(), Mode::NONE).expect("parse");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file]);
    assert!(check.errors.is_empty(), "SRC must type-check: {:?}", check.errors);
    check
}

#[test]
fn every_arm_finds_a_free_type_parameter() {
    let mut check = checked();
    for name in [
        "freeSlice",
        "freeArray",
        "freePointer",
        "freeChan",
        "freeMapKey",
        "freeMapElem",
        "freeStruct",
        "freeSignature",
        "freeNamed",
        "freeNamedIface",
        "freeAnonIface",
        "freeNested",
        "freeParam",
        "freeAlias",
    ] {
        let t = param_type(&check, name);
        let mut types = check.types.clone();
        assert!(
            has_free_type_param(&mut types, &check.objects, &check.packages, t),
            "{name}'s parameter mentions a free type parameter"
        );
        check.types = types;
    }
}

#[test]
fn the_same_shapes_over_int_are_not_free() {
    let mut check = checked();
    for name in [
        "fixedSlice",
        "fixedArray",
        "fixedPointer",
        "fixedChan",
        "fixedMapKey",
        "fixedMapElem",
        "fixedStruct",
        "fixedSignature",
        "fixedNamed",
        "fixedNamedIface",
        "fixedAnonIface",
        "fixedNested",
        "fixedBasic",
        "fixedAlias",
        // A constraint interface with a union term set: the terms are
        // concrete, so nothing is free — but the arm has to walk them.
        "fixedConstraint",
    ] {
        let t = param_type(&check, name);
        let mut types = check.types.clone();
        assert!(
            !has_free_type_param(&mut types, &check.objects, &check.packages, t),
            "{name}'s parameter has no free type parameter"
        );
        check.types = types;
    }
}

/// The memo is also the cycle breaker: a type that refers to itself answers
/// once rather than recursing forever.
#[test]
fn a_recursive_type_terminates() {
    let fset = FileSet::new();
    let file = parse_file(
        &fset,
        "p.go",
        b"package p\n\ntype Node[T any] struct {\n\tV    T\n\tNext *Node[T]\n}\n\nfunc f[T any](x Node[T]) {}\n\nfunc g(x Node[int])  {}\n",
        Mode::NONE,
    )
    .expect("parse");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file]);
    assert!(check.errors.is_empty(), "{:?}", check.errors);

    let mut types = check.types.clone();
    let mut free = Free::new();
    let t = param_type(&check, "f");
    assert!(free.has(&mut types, &check.objects, &check.packages, t));
    let u = param_type(&check, "g");
    assert!(!free.has(&mut types, &check.objects, &check.packages, u));
}
