//! Tests for `decl.rs` (chunk 23a) — type declarations via the Checker.
//!
//! Each test parses a small Go source, runs `collect_objects`, then forces
//! `obj_decl` on a type name and inspects the resulting type.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::arena::TypeData;
use guff_types::scope::lookup as scope_lookup;
use guff_types::{Checker, Config, TypeKind};

fn parse(src: &str) -> guff::ast::File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

/// Build a checker, collect objects, and return it (objects not yet decl'd).
fn collect(src: &str) -> Checker {
    let mut check = Checker::new(Config::default());
    check.files = vec![parse(src)];
    check.collect_objects();
    check
}

/// Look up a package-scope type name and force its declaration.
fn decl_type(check: &mut Checker, name: &str) -> guff_types::TypeId {
    let pkg_scope = check.packages.get(check.pkg).scope();
    let obj = scope_lookup(&check.scopes, pkg_scope, name).expect("type name declared");
    check.obj_decl(obj);
    obj.typ(&check.objects).expect("type set after obj_decl")
}

#[test]
fn defined_type_over_basic() {
    let mut check = collect("package p\ntype T int\n");
    let t = decl_type(&mut check, "T");
    // T is a Named type whose underlying is the basic int.
    assert!(matches!(check.types.get(t), TypeData::Named(_)));
    assert_eq!(
        t.underlying(&check.types).kind(&check.types),
        TypeKind::Basic
    );
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
}

#[test]
fn defined_type_over_slice() {
    let mut check = collect("package p\ntype S []int\n");
    let s = decl_type(&mut check, "S");
    assert!(matches!(check.types.get(s), TypeData::Named(_)));
    assert_eq!(
        s.underlying(&check.types).kind(&check.types),
        TypeKind::Slice
    );
}

#[test]
fn defined_type_over_map_and_chan() {
    let mut check = collect("package p\ntype M map[string]int\ntype C chan int\n");
    let m = decl_type(&mut check, "M");
    let c = decl_type(&mut check, "C");
    assert_eq!(m.underlying(&check.types).kind(&check.types), TypeKind::Map);
    assert_eq!(
        c.underlying(&check.types).kind(&check.types),
        TypeKind::Chan
    );
}

#[test]
fn defined_type_over_another_named() {
    // `type U int; type T U` — T and U share the underlying int.
    let mut check = collect("package p\ntype U int\ntype T U\n");
    let t = decl_type(&mut check, "T");
    assert!(matches!(check.types.get(t), TypeData::Named(_)));
    assert_eq!(
        t.underlying(&check.types).kind(&check.types),
        TypeKind::Basic
    );
}

#[test]
fn forward_reference_is_resolved_via_obj_decl() {
    // T references S declared later in the file; obj_decl(T) must force S.
    let mut check = collect("package p\ntype T S\ntype S []int\n");
    let t = decl_type(&mut check, "T");
    assert_eq!(
        t.underlying(&check.types).kind(&check.types),
        TypeKind::Slice
    );
    // S got resolved as a side effect.
    let pkg_scope = check.packages.get(check.pkg).scope();
    let s = scope_lookup(&check.scopes, pkg_scope, "S").unwrap();
    assert!(s.typ(&check.objects).is_some());
}

#[test]
fn alias_declaration() {
    // `type A = int` — A is an Alias to int (not a Named).
    let mut check = collect("package p\ntype A = int\n");
    let a = decl_type(&mut check, "A");
    assert!(matches!(check.types.get(a), TypeData::Alias(_)));
    // Unaliased underlying is the basic int.
    assert_eq!(
        a.underlying(&check.types).kind(&check.types),
        TypeKind::Basic
    );
}

// ---- chunk 23b: const / var declarations ----

use guff_constant::int64_val;
use guff_types::arena::ObjectData;

fn lookup(check: &Checker, name: &str) -> guff_types::ObjectId {
    let pkg_scope = check.packages.get(check.pkg).scope();
    scope_lookup(&check.scopes, pkg_scope, name).expect("declared")
}

#[test]
fn const_without_type_adopts_default() {
    let mut check = collect("package p\nconst c = 5\n");
    let c = lookup(&check, "c");
    check.obj_decl(c);
    let t = c.typ(&check.objects).unwrap();
    assert_eq!(t.kind(&check.types), TypeKind::Basic); // int
    if let ObjectData::Const(cd) = check.objects.get(c) {
        assert_eq!(int64_val(cd.val()).0, 5);
    } else {
        panic!("not a const");
    }
    assert!(check.errors.is_empty(), "errors: {:?}", check.errors);
}

#[test]
fn const_with_explicit_type() {
    let mut check = collect("package p\nconst c int8 = 100\n");
    let c = lookup(&check, "c");
    check.obj_decl(c);
    if let ObjectData::Const(cd) = check.objects.get(c) {
        assert_eq!(int64_val(cd.val()).0, 100);
    }
    assert!(check.errors.is_empty(), "errors: {:?}", check.errors);
}

#[test]
fn const_overflow_reports_error() {
    let mut check = collect("package p\nconst c int8 = 1000\n");
    let c = lookup(&check, "c");
    check.obj_decl(c);
    assert!(!check.errors.is_empty(), "expected overflow error");
}

#[test]
fn var_without_type_infers_from_init() {
    let mut check = collect("package p\nvar v = 3\n");
    let v = lookup(&check, "v");
    check.obj_decl(v);
    let t = v.typ(&check.objects).unwrap();
    assert_eq!(t.kind(&check.types), TypeKind::Basic);
    assert!(check.errors.is_empty(), "errors: {:?}", check.errors);
}

#[test]
fn var_with_binary_init() {
    let mut check = collect("package p\nvar v = 1 + 2\n");
    let v = lookup(&check, "v");
    check.obj_decl(v);
    assert!(v.typ(&check.objects).is_some());
    assert!(check.errors.is_empty(), "errors: {:?}", check.errors);
}

#[test]
fn var_with_explicit_type_and_init() {
    let mut check = collect("package p\nvar v int = 5\n");
    let v = lookup(&check, "v");
    check.obj_decl(v);
    let t = v.typ(&check.objects).unwrap();
    assert_eq!(t.kind(&check.types), TypeKind::Basic);
    assert!(check.errors.is_empty(), "errors: {:?}", check.errors);
}

#[test]
fn func_decl_builds_signature() {
    let mut check = collect("package p\nfunc f(a int) bool { return true }\n");
    let f = decl_type(&mut check, "f"); // works for any pkg-scope object
                                        // f's type is a Signature with 1 param and 1 result.
    assert_eq!(f.kind(&check.types), TypeKind::Signature);
    let params = guff_types::signature::signature_params(&check.types, f);
    let results = guff_types::signature::signature_results(&check.types, f);
    assert_eq!(guff_types::tuple::tuple_len(&check.types, params), 1);
    assert_eq!(guff_types::tuple::tuple_len(&check.types, results), 1);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
}

#[test]
fn methods_attached_to_named_type() {
    let mut check = collect("package p\ntype T int\nfunc (t T) M() {}\nfunc (t *T) P() {}\n");
    let t = decl_type(&mut check, "T");
    // collectMethods (run by obj_decl) attached both methods to T.
    let n = guff_types::named::named_num_methods(&check.types, t);
    assert_eq!(n, 2, "M and P attached to T");
    let names: Vec<&str> = (0..n)
        .map(|i| guff_types::named::named_method(&check.types, t, i).name(&check.objects))
        .collect();
    assert!(names.contains(&"M"));
    assert!(names.contains(&"P"));
    // check.methods entry consumed (keyed by the TypeName object).
    let pkg_scope = check.packages.get(check.pkg).scope();
    let t_obj = scope_lookup(&check.scopes, pkg_scope, "T").unwrap();
    assert!(check.methods.get(&t_obj).is_none());
}
