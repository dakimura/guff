//! Tests for `resolver.rs` (chunk 22) — package-level object collection.
//!
//! Each test parses a small Go source with `guff::parser`, hands the
//! resulting `File` to a fresh `Checker`, runs `collect_objects`, and inspects
//! the package scope / `obj_map` / `methods`.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::arena::ObjectData;
use guff_types::scope::lookup as scope_lookup;
use guff_types::{Checker, Config};

/// Parse `src` into a single `File`, panicking on parse errors.
fn parse(src: &str) -> guff::ast::File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

/// Build a checker, collect objects from `src`, and return it.
fn collect(src: &str) -> Checker {
    let mut check = Checker::new(Config::default());
    check.files = vec![parse(src)];
    check.collect_objects();
    check
}

#[test]
fn collects_var_const_func_into_package_scope() {
    let check = collect("package p\nvar x int\nconst c = 1\nfunc f() {}\n");
    let pkg_scope = check.packages.get(check.pkg).scope();

    let x = scope_lookup(&check.scopes, pkg_scope, "x").expect("x declared");
    let c = scope_lookup(&check.scopes, pkg_scope, "c").expect("c declared");
    let f = scope_lookup(&check.scopes, pkg_scope, "f").expect("f declared");

    assert!(matches!(check.objects.get(x), ObjectData::Var(_)));
    assert!(matches!(check.objects.get(c), ObjectData::Const(_)));
    assert!(matches!(check.objects.get(f), ObjectData::Func(_)));

    // Each got an obj_map entry and a non-zero source order.
    assert!(check.obj_map.contains_key(&x));
    assert!(check.obj_map.contains_key(&c));
    assert!(check.obj_map.contains_key(&f));
    assert_eq!(check.obj_map.len(), 3);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);

    // Package name adopted from the package clause.
    assert_eq!(check.packages.get(check.pkg).name(), "p");
}

#[test]
fn collects_type_decl() {
    let check = collect("package p\ntype T int\n");
    let pkg_scope = check.packages.get(check.pkg).scope();
    let t = scope_lookup(&check.scopes, pkg_scope, "T").expect("T declared");
    assert!(matches!(check.objects.get(t), ObjectData::TypeName(_)));
    // tdecl recorded for later objDecl.
    assert!(check.obj_map.get(&t).unwrap().tdecl.is_some());
}

#[test]
fn const_group_iota_and_inheritance() {
    // Inherited init: B inherits "iota" expression; values track iota index.
    let check = collect("package p\nconst (\n\tA = iota\n\tB\n\tC\n)\n");
    let pkg_scope = check.packages.get(check.pkg).scope();
    for name in ["A", "B", "C"] {
        let o = scope_lookup(&check.scopes, pkg_scope, name)
            .unwrap_or_else(|| panic!("{name} declared"));
        assert!(matches!(check.objects.get(o), ObjectData::Const(_)));
    }
    // B and C inherit their init expression from A.
    let b = scope_lookup(&check.scopes, pkg_scope, "B").unwrap();
    let di = check.obj_map.get(&b).unwrap();
    assert!(di.inherited, "B should inherit A's init expr");
    assert!(di.init.is_some(), "B has an inherited init expr");
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
}

#[test]
fn methods_associated_with_receiver_base_type() {
    let check =
        collect("package p\ntype T int\nfunc (t T) M() {}\nfunc (t *T) P() {}\nfunc Free() {}\n");
    let pkg_scope = check.packages.get(check.pkg).scope();

    // The base type name T.
    let t = scope_lookup(&check.scopes, pkg_scope, "T").expect("T declared");
    // Methods are NOT in the package scope.
    assert!(scope_lookup(&check.scopes, pkg_scope, "M").is_none());
    assert!(scope_lookup(&check.scopes, pkg_scope, "P").is_none());
    // Free function IS in the package scope.
    assert!(scope_lookup(&check.scopes, pkg_scope, "Free").is_some());

    // Both methods associated with T.
    let ms = check.methods.get(&t).expect("T has methods");
    assert_eq!(ms.len(), 2, "M and P associated with T");
    let names: Vec<&str> = ms.iter().map(|m| m.name(&check.objects)).collect();
    assert!(names.contains(&"M"));
    assert!(names.contains(&"P"));
}

#[test]
fn duplicate_decl_reports_error() {
    let check = collect("package p\nvar x int\nvar x int\n");
    assert!(
        check.errors.iter().any(|e| e.msg.contains("redeclared")),
        "expected a redeclared error, got {:?}",
        check.errors
    );
}

#[test]
fn sort_objects_orders_by_source() {
    let mut check = collect("package p\nvar a int\nvar b int\nfunc c() {}\n");
    check.sort_objects();
    // obj_list is sorted by source order; orders are 1..=3 ascending.
    let orders: Vec<u32> = check
        .obj_list
        .iter()
        .map(|o| o.order(&check.objects))
        .collect();
    let mut sorted = orders.clone();
    sorted.sort();
    assert_eq!(orders, sorted);
    assert_eq!(check.obj_list.len(), 3);
}
