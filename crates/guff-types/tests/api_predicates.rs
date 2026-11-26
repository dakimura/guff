//! Chunk-54 tests: `api_predicates.rs` — the public type-relation predicates
//! (`AssignableTo` / `ConvertibleTo` / `Implements` / `Satisfies` /
//! `AssertableTo` / `Identical` / `IdenticalIgnoreTags`), port of Go's
//! `api_predicates.go`.

use guff_types::{
    add_method, api_assertable_to, api_assignable_to, api_convertible_to, api_identical,
    api_identical_ignore_tags, api_implements, api_satisfies, init_universe_full, new_field,
    new_func, new_interface_type, new_named, new_param, new_signature_type, new_slice, new_struct,
    new_type_name, BasicKind, TypeId, Universe,
};

fn b(u: &Universe, k: BasicKind) -> TypeId {
    u.typ[k as usize]
}

/// Build `type <name> struct{}` with a value-receiver method `func (name) <method>()`.
fn named_with_method(u: &mut Universe, name: &str, method: &str) -> TypeId {
    let empty = new_struct(&mut u.type_arena, vec![], vec![]);
    let tn = new_type_name(&mut u.object_arena, name, None);
    let t = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn,
        Some(empty),
        vec![],
    );
    let recv = new_param(&mut u.object_arena, "r", t);
    let sig = new_signature_type(&mut u.type_arena, Some(recv), &[], &[], None, None, false);
    let m = new_func(&mut u.object_arena, method, Some(sig));
    add_method(&mut u.type_arena, &u.object_arena, t, m);
    t
}

/// Build `interface { <method>() }`.
fn iface_with_method(u: &mut Universe, method: &str) -> TypeId {
    let sig = new_signature_type(&mut u.type_arena, None, &[], &[], None, None, false);
    let m = new_func(&mut u.object_arena, method, Some(sig));
    new_interface_type(&mut u.type_arena, vec![m], vec![])
}

// ---------------------------------------------------------------- AssignableTo

#[test]
fn assignable_identical_basic() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    assert!(api_assignable_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        int,
        int
    ));
}

#[test]
fn assignable_distinct_basic_false() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let s = b(&u, BasicKind::String);
    assert!(!api_assignable_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        int,
        s
    ));
}

#[test]
fn assignable_unnamed_to_named_underlying() {
    // type S []int — an unnamed []int is assignable to S (identical underlying,
    // one operand unnamed).
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let slice = new_slice(&mut u.type_arena, int);
    let tn = new_type_name(&mut u.object_arena, "S", None);
    let s = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn,
        Some(slice),
        vec![],
    );
    assert!(api_assignable_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        slice,
        s
    ));
}

#[test]
fn assignable_to_interface_implemented() {
    let mut u = init_universe_full();
    let t = named_with_method(&mut u, "T", "M");
    let iface = iface_with_method(&mut u, "M");
    assert!(api_assignable_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        t,
        iface
    ));
}

#[test]
fn assignable_to_interface_not_implemented() {
    let mut u = init_universe_full();
    let t = named_with_method(&mut u, "T", "M");
    let iface = iface_with_method(&mut u, "N"); // requires N; T only has M
    assert!(!api_assignable_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        t,
        iface
    ));
}

// --------------------------------------------------------------- ConvertibleTo

#[test]
fn convertible_int_to_float() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let f = b(&u, BasicKind::Float64);
    assert!(api_convertible_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        int,
        f
    ));
}

#[test]
fn convertible_bytes_to_string() {
    let mut u = init_universe_full();
    let byte = b(&u, BasicKind::Uint8);
    let bytes = new_slice(&mut u.type_arena, byte);
    let s = b(&u, BasicKind::String);
    assert!(api_convertible_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        bytes,
        s
    ));
}

#[test]
fn convertible_struct_to_int_false() {
    let mut u = init_universe_full();
    let empty = new_struct(&mut u.type_arena, vec![], vec![]);
    let int = b(&u, BasicKind::Int);
    assert!(!api_convertible_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        empty,
        int
    ));
}

// ------------------------------------------------------------------ Implements

#[test]
fn implements_true() {
    let mut u = init_universe_full();
    let t = named_with_method(&mut u, "T", "M");
    let iface = iface_with_method(&mut u, "M");
    assert!(api_implements(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        t,
        iface
    ));
}

#[test]
fn implements_false() {
    let mut u = init_universe_full();
    let t = named_with_method(&mut u, "T", "M");
    let iface = iface_with_method(&mut u, "N");
    assert!(!api_implements(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        t,
        iface
    ));
}

#[test]
fn implements_empty_interface_always_true() {
    // Every type implements the empty interface.
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let empty = new_interface_type(&mut u.type_arena, vec![], vec![]);
    assert!(api_implements(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        int,
        empty
    ));
}

// ------------------------------------------------------------------- Satisfies

#[test]
fn satisfies_constraint() {
    let mut u = init_universe_full();
    let t = named_with_method(&mut u, "T", "M");
    let iface = iface_with_method(&mut u, "M");
    assert!(api_satisfies(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        t,
        iface
    ));
}

// ----------------------------------------------------------------- AssertableTo

#[test]
fn assertable_to_concrete_with_methods() {
    // var i I; i.(T) where T has all of I's methods.
    let mut u = init_universe_full();
    let iface = iface_with_method(&mut u, "M"); // I requires M
    let t = named_with_method(&mut u, "T", "M"); // T has M
    assert!(api_assertable_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        iface,
        t
    ));
}

#[test]
fn assertable_to_concrete_missing_method_false() {
    let mut u = init_universe_full();
    let iface = iface_with_method(&mut u, "M");
    let t = named_with_method(&mut u, "T", "N"); // T has N, not M
    assert!(!api_assertable_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        iface,
        t
    ));
}

#[test]
fn assertable_to_interface_always_true() {
    // Asserting an interface value to another interface type needs no static check.
    let mut u = init_universe_full();
    let iface = iface_with_method(&mut u, "M");
    let iface2 = iface_with_method(&mut u, "X");
    assert!(api_assertable_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        iface,
        iface2
    ));
}

// -------------------------------------------------- Identical / IgnoreTags

#[test]
fn identical_slices() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let a = new_slice(&mut u.type_arena, int);
    let c = new_slice(&mut u.type_arena, int);
    assert!(api_identical(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        a,
        c
    ));
}

#[test]
fn identical_distinct_slices_false() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let s = b(&u, BasicKind::String);
    let a = new_slice(&mut u.type_arena, int);
    let c = new_slice(&mut u.type_arena, s);
    assert!(!api_identical(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        a,
        c
    ));
}

#[test]
fn identical_ignore_tags() {
    // struct{ x int `a` } vs struct{ x int `b` }: differ with tags, identical
    // when tags are ignored.
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let f1 = new_field(&mut u.object_arena, "x", int, false);
    let s1 = new_struct(&mut u.type_arena, vec![f1], vec!["a".to_string()]);
    let f2 = new_field(&mut u.object_arena, "x", int, false);
    let s2 = new_struct(&mut u.type_arena, vec![f2], vec!["b".to_string()]);

    assert!(!api_identical(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        s1,
        s2
    ));
    assert!(api_identical_ignore_tags(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        s1,
        s2
    ));
}

#[test]
fn implements_slice_uint8_not_error() {
    let mut u = init_universe_full();
    let uint8 = b(&u, BasicKind::Uint8);
    let slice = new_slice(&mut u.type_arena, uint8);
    let err = u.error;
    assert!(
        !api_implements(
            &mut u.type_arena,
            &u.object_arena,
            &u.package_arena,
            slice,
            err
        ),
        "[]uint8 must not implement error"
    );
    let ptr = guff_types::new_pointer(&mut u.type_arena, slice);
    assert!(
        !api_implements(
            &mut u.type_arena,
            &u.object_arena,
            &u.package_arena,
            ptr,
            err
        ),
        "*[]uint8 must not implement error"
    );
}
