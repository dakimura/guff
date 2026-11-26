//! An untyped rune defaults to `universeRune`: the basic spelled `rune`,
//! identical to `int32` but printed as `rune`. printf's `has arg 'x' of wrong
//! type rune` is where this shows; guff printed `int32`.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_types::arena::ObjectData;
use guff_types::{Checker, Config};

/// The printed type of package-level var `name` in `src`.
fn var_type(src: &str, name: &str) -> String {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file]);
    assert!(check.errors.is_empty(), "{:?}", check.errors);
    let obj = check
        .info
        .defs
        .values()
        .flatten()
        .copied()
        .find(|&o| matches!(check.objects.get(o), ObjectData::Var(v) if v.name() == name))
        .expect("var declared");
    let ObjectData::Var(v) = check.objects.get(obj) else {
        unreachable!()
    };
    guff_types::typestring::type_string(&check.types, &check.objects, &check.packages, v.typ(), None)
}

#[test]
fn untyped_rune_defaults_to_rune() {
    assert_eq!(var_type("package p\nvar v = 'x'\n", "v"), "rune");
}

#[test]
fn spelled_int32_stays_int32() {
    assert_eq!(var_type("package p\nvar v = int32('x')\n", "v"), "int32");
}

#[test]
fn other_untyped_constants_are_unchanged() {
    assert_eq!(var_type("package p\nvar v = 1\n", "v"), "int");
    assert_eq!(var_type("package p\nvar v = 1.5\n", "v"), "float64");
}
