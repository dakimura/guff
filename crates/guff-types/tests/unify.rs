//! Chunk-12 tests: `unify.rs` — Unifier core, structural unification.

use guff_types::{
    bind_tparams, init_universe_full, named_set_type_params, new_chan, new_field, new_map,
    new_named, new_param, new_pointer, new_signature_type, new_slice, new_struct, new_tuple,
    new_type_name, new_type_param, set_constraint, unify, BasicKind, ChanDir, Unifier, UnifyMode,
};

// ----------------------------------------------------------------------------
// Basic / Pointer / Slice / Map / Chan

#[test]
fn unify_same_basic_returns_true() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let mut uni = Unifier::new(&[], &[], false);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        int,
        int,
        UnifyMode::ZERO,
    ));
}

#[test]
fn unify_different_basics_returns_false() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let s = u.typ[BasicKind::String as usize];
    let mut uni = Unifier::new(&[], &[], false);
    assert!(!unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        int,
        s,
        UnifyMode::ZERO,
    ));
}

#[test]
fn unify_two_slices_via_element_recursion() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let s1 = new_slice(&mut u.type_arena, int);
    let s2 = new_slice(&mut u.type_arena, int);
    let mut uni = Unifier::new(&[], &[], false);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        s1,
        s2,
        UnifyMode::ZERO,
    ));
}

#[test]
fn unify_map_key_and_value() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let s = u.typ[BasicKind::String as usize];
    let m1 = new_map(&mut u.type_arena, int, s);
    let m2 = new_map(&mut u.type_arena, int, s);
    let mut uni = Unifier::new(&[], &[], false);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        m1,
        m2,
        UnifyMode::ZERO,
    ));
    let m3 = new_map(&mut u.type_arena, int, int);
    assert!(!unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        m1,
        m3,
        UnifyMode::ZERO,
    ));
}

#[test]
fn unify_chan_direction_only_checked_in_exact_mode() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let bidir = new_chan(&mut u.type_arena, ChanDir::SendRecv, int);
    let send = new_chan(&mut u.type_arena, ChanDir::SendOnly, int);
    let mut uni = Unifier::new(&[], &[], false);
    // Inexact: directions ignored ⇒ unify.
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        bidir,
        send,
        UnifyMode::ZERO,
    ));
    // Exact: directions matter ⇒ fail.
    assert!(!unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        bidir,
        send,
        UnifyMode::EXACT,
    ));
}

// ----------------------------------------------------------------------------
// TypeParam inference

#[test]
fn unify_unbound_tparam_against_concrete_records_inference() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    // type T any
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);

    let mut uni = Unifier::new(&[tp], &[None], false);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        tp,
        int,
        UnifyMode::ZERO,
    ));
    assert_eq!(uni.at(tp), Some(int));
    assert_eq!(uni.unknowns(), 0);
}

#[test]
fn unify_tparam_with_conflicting_concrete_fails() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let s = u.typ[BasicKind::String as usize];
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);

    let mut uni = Unifier::new(&[tp], &[None], false);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        tp,
        int,
        UnifyMode::ZERO,
    ));
    // Now bind to a different concrete → must fail.
    assert!(!unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        tp,
        s,
        UnifyMode::ZERO,
    ));
}

#[test]
fn unify_through_slice_infers_inner_tparam() {
    // unify([]T, []int) ⇒ T := int
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);
    let slice_t = new_slice(&mut u.type_arena, tp);
    let slice_int = new_slice(&mut u.type_arena, int);

    let mut uni = Unifier::new(&[tp], &[None], false);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        slice_t,
        slice_int,
        UnifyMode::ZERO,
    ));
    assert_eq!(uni.at(tp), Some(int));
}

#[test]
fn unify_join_two_tparams_then_infer_one() {
    // unify(T, U) → join. unify(T, int) → both = int.
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let tn_t = new_type_name(&mut u.object_arena, "T", None);
    let tn_v = new_type_name(&mut u.object_arena, "V", None);
    let tp_t = new_type_param(&mut u.type_arena, tn_t, None);
    let tp_v = new_type_param(&mut u.type_arena, tn_v, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp_t, tp_v]);

    let mut uni = Unifier::new(&[tp_t, tp_v], &[None, None], false);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        tp_t,
        tp_v,
        UnifyMode::ZERO,
    ));
    // Both still None — but joined.
    assert_eq!(uni.at(tp_t), None);
    assert_eq!(uni.at(tp_v), None);
    // Now set T = int via unify.
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        tp_t,
        int,
        UnifyMode::ZERO,
    ));
    assert_eq!(uni.at(tp_t), Some(int));
    assert_eq!(uni.at(tp_v), Some(int));
}

#[test]
fn unify_join_then_fail_when_both_inferred_differently() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let s = u.typ[BasicKind::String as usize];
    let tn_t = new_type_name(&mut u.object_arena, "T", None);
    let tn_v = new_type_name(&mut u.object_arena, "V", None);
    let tp_t = new_type_param(&mut u.type_arena, tn_t, None);
    let tp_v = new_type_param(&mut u.type_arena, tn_v, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp_t, tp_v]);

    let mut uni = Unifier::new(&[tp_t, tp_v], &[Some(int), Some(s)], false);
    // Both already inferred to different concretes → join fails → unify fails.
    assert!(!unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        tp_t,
        tp_v,
        UnifyMode::ZERO,
    ));
}

#[test]
fn unify_inexact_with_named_prefers_defined_type() {
    // T already inferred to []int. Unify against `type S []int` (a Named).
    // Inexact mode: T should be re-set to S.
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let slice_int = new_slice(&mut u.type_arena, int);
    let tn_s = new_type_name(&mut u.object_arena, "S", None);
    let named_s = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn_s,
        Some(slice_int),
        vec![],
    );

    let tn_t = new_type_name(&mut u.object_arena, "T", None);
    let tp_t = new_type_param(&mut u.type_arena, tn_t, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp_t]);

    let mut uni = Unifier::new(&[tp_t], &[Some(slice_int)], false);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        tp_t,
        named_s,
        UnifyMode::ZERO,
    ));
    // T should now be S (the defined type), not the slice literal.
    assert_eq!(uni.at(tp_t), Some(named_s));
}

// ----------------------------------------------------------------------------
// Struct / Signature / Named (with args)

#[test]
fn unify_structs_with_matching_field_names_and_types() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let f1 = new_field(&mut u.object_arena, "X", int, false);
    let f2 = new_field(&mut u.object_arena, "X", int, false);
    let s1 = new_struct(&mut u.type_arena, vec![f1], vec![String::new()]);
    let s2 = new_struct(&mut u.type_arena, vec![f2], vec![String::new()]);
    let mut uni = Unifier::new(&[], &[], false);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        s1,
        s2,
        UnifyMode::ZERO,
    ));
}

#[test]
fn unify_signatures_must_have_matching_variadic_and_params() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let p1 = new_param(&mut u.object_arena, "x", int);
    let params1 = new_tuple(&mut u.type_arena, &[p1]);
    let sig1 = new_signature_type(&mut u.type_arena, None, &[], &[], params1, None, false);
    let p2 = new_param(&mut u.object_arena, "y", int);
    let params2 = new_tuple(&mut u.type_arena, &[p2]);
    let sig2 = new_signature_type(&mut u.type_arena, None, &[], &[], params2, None, false);
    let mut uni = Unifier::new(&[], &[], false);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        sig1,
        sig2,
        UnifyMode::ZERO,
    ));

    // Variadic mismatch fails.
    let sig3 = new_signature_type(&mut u.type_arena, None, &[], &[], params1, None, true);
    assert!(!unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        sig1,
        sig3,
        UnifyMode::ZERO,
    ));
}

#[test]
fn unify_pointer_struct_recursive() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let f1 = new_field(&mut u.object_arena, "X", int, false);
    let s1 = new_struct(&mut u.type_arena, vec![f1], vec![String::new()]);
    let p1 = new_pointer(&mut u.type_arena, s1);

    let f2 = new_field(&mut u.object_arena, "X", int, false);
    let s2 = new_struct(&mut u.type_arena, vec![f2], vec![String::new()]);
    let p2 = new_pointer(&mut u.type_arena, s2);

    let mut uni = Unifier::new(&[], &[], false);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        p1,
        p2,
        UnifyMode::ZERO,
    ));
}

#[test]
fn unify_nameds_must_share_origin_typename() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let tn_a = new_type_name(&mut u.object_arena, "A", None);
    let a = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn_a,
        Some(int),
        vec![],
    );
    // A different Named with same name/underlying — must not unify.
    let tn_b = new_type_name(&mut u.object_arena, "A", None);
    let b = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn_b,
        Some(int),
        vec![],
    );

    let mut uni = Unifier::new(&[], &[], false);
    assert!(!unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        a,
        b,
        UnifyMode::ZERO,
    ));
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        a,
        a,
        UnifyMode::ZERO,
    ));
}

#[test]
fn unify_inexact_named_vs_literal_unwraps_named() {
    // type S []int. Unify S vs []int inexactly ⇒ true.
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let slice_int = new_slice(&mut u.type_arena, int);
    let tn_s = new_type_name(&mut u.object_arena, "S", None);
    let s = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn_s,
        Some(slice_int),
        vec![],
    );

    let mut uni = Unifier::new(&[], &[], false);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        s,
        slice_int,
        UnifyMode::ZERO,
    ));
    // Exact mode: must fail.
    assert!(!unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        s,
        slice_int,
        UnifyMode::EXACT,
    ));
}

// ----------------------------------------------------------------------------
// Public Unifier accessors

#[test]
fn unifier_at_set_unknowns_inferred() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);

    let mut uni = Unifier::new(&[tp], &[None], false);
    assert!(uni.at(tp).is_none());
    assert_eq!(uni.unknowns(), 1);
    uni.set(tp, int);
    assert_eq!(uni.at(tp), Some(int));
    assert_eq!(uni.unknowns(), 0);
    let list = uni.inferred(&[tp]);
    assert_eq!(list, vec![Some(int)]);
}

// ----------------------------------------------------------------------------
// Interface inference (chunk 63 — enable_interface_inference = true)

/// Build `interface { <method>() }` (a niladic, result-less method).
fn iface_with_methods(
    u: &mut guff_types::Universe,
    methods: &[&str],
) -> guff_types::TypeId {
    let mut fs = Vec::new();
    for m in methods {
        let sig = new_signature_type(&mut u.type_arena, None, &[], &[], None, None, false);
        fs.push(guff_types::new_func(
            &mut u.object_arena,
            *m,
            Some(sig),
        ));
    }
    guff_types::new_interface_type(&mut u.type_arena, fs, vec![])
}

#[test]
fn unify_interfaces_subset_methods_with_inference() {
    // interface{Foo()} vs interface{Foo(); Bar()}: the smaller method set is a
    // subset and the common method (Foo) unifies, so with interface inference
    // on they unify.
    let mut u = init_universe_full();
    let small = iface_with_methods(&mut u, &["Foo"]);
    let large = iface_with_methods(&mut u, &["Foo", "Bar"]);

    let mut uni = Unifier::new(&[], &[], true);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        small,
        large,
        UnifyMode::ASSIGN,
    ));
}

#[test]
fn unify_interfaces_subset_without_inference_fails() {
    // The same two distinct interfaces do NOT unify structurally when interface
    // inference is off (their method sets differ).
    let mut u = init_universe_full();
    let small = iface_with_methods(&mut u, &["Foo"]);
    let large = iface_with_methods(&mut u, &["Foo", "Bar"]);

    let mut uni = Unifier::new(&[], &[], false);
    assert!(!unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        small,
        large,
        UnifyMode::ASSIGN,
    ));
}

#[test]
fn unify_interfaces_missing_method_fails_with_inference() {
    // interface{Bar()} vs interface{Foo()}: the subset (one method) is not
    // present in the other, so unification fails even with inference on.
    let mut u = init_universe_full();
    let a = iface_with_methods(&mut u, &["Bar"]);
    let b = iface_with_methods(&mut u, &["Foo"]);

    let mut uni = Unifier::new(&[], &[], true);
    assert!(!unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        a,
        b,
        UnifyMode::ASSIGN,
    ));
}

#[test]
fn unify_interface_with_concrete_implementer() {
    // interface{Foo()} unifies with a named type that has method Foo() when
    // interface inference is on (single-interface branch via LookupFieldOrMethod).
    let mut u = init_universe_full();
    let iface = iface_with_methods(&mut u, &["Foo"]);

    // type T struct{} with method Foo().
    let empty = new_struct(&mut u.type_arena, vec![], vec![]);
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let t = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn,
        Some(empty),
        vec![],
    );
    let recv = new_param(&mut u.object_arena, "r", t);
    let sig = new_signature_type(&mut u.type_arena, Some(recv), &[], &[], None, None, false);
    let m = guff_types::new_func(&mut u.object_arena, "Foo", Some(sig));
    guff_types::add_method(&mut u.type_arena, &u.object_arena, t, m);

    let mut uni = Unifier::new(&[], &[], true);
    assert!(unify(
        &mut uni,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        iface,
        t,
        UnifyMode::ASSIGN,
    ));

    // A type lacking Foo() does not unify with the interface.
    let empty2 = new_struct(&mut u.type_arena, vec![], vec![]);
    let tn2 = new_type_name(&mut u.object_arena, "U", None);
    let t2 = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn2,
        Some(empty2),
        vec![],
    );
    let mut uni2 = Unifier::new(&[], &[], true);
    assert!(!unify(
        &mut uni2,
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        iface,
        t2,
        UnifyMode::ASSIGN,
    ));
}

// Silence unused helpers warning.
#[test]
fn unused_imports_smoke() {
    let _ = named_set_type_params;
    let _ = set_constraint;
}
