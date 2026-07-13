//! Chunk-4 tests: the type-set machinery exercised through `Interface` and
//! `TypeParam` accessors.

use guff_types::{
    init_universe, interface_compute_typeset, interface_empty, interface_is_comparable,
    interface_is_method_set, interface_method, interface_num_methods, interface_typeset, new_func,
    new_interface_type, new_signature_type, new_term, new_tuple, new_type_name, new_type_param,
    new_union, new_var, set_constraint, type_param_iface, BasicKind, ObjectArena, PackageArena,
    TypeKind,
};

#[test]
fn empty_interface_typeset() {
    let mut t_arena = init_universe().0;
    let o_arena = ObjectArena::new();
    let iface = new_interface_type(&mut t_arena, vec![], vec![]);

    assert!(interface_empty(
        &mut t_arena,
        &o_arena,
        &PackageArena::new(),
        iface
    ));
    assert!(interface_is_method_set(
        &mut t_arena,
        &o_arena,
        &PackageArena::new(),
        iface
    ));
    assert!(!interface_is_comparable(
        &mut t_arena,
        &o_arena,
        &PackageArena::new(),
        iface
    ));
    assert_eq!(
        interface_num_methods(&mut t_arena, &o_arena, &PackageArena::new(), iface),
        0
    );
}

#[test]
fn interface_with_methods_only() {
    // interface { Foo(); Bar() }
    let (mut t_arena, _) = init_universe();
    let mut o_arena = ObjectArena::new();

    let sig = new_signature_type(&mut t_arena, None, &[], &[], None, None, false);
    let foo = new_func(&mut o_arena, "Foo", Some(sig));
    let bar = new_func(&mut o_arena, "Bar", Some(sig));
    let iface = new_interface_type(&mut t_arena, vec![foo, bar], vec![]);

    assert!(!interface_empty(
        &mut t_arena,
        &o_arena,
        &PackageArena::new(),
        iface
    ));
    assert!(interface_is_method_set(
        &mut t_arena,
        &o_arena,
        &PackageArena::new(),
        iface
    ));
    assert_eq!(
        interface_num_methods(&mut t_arena, &o_arena, &PackageArena::new(), iface),
        2
    );
    // Methods are sorted by name in our chunk-4 stub (Bar < Foo).
    assert_eq!(
        interface_method(&mut t_arena, &o_arena, &PackageArena::new(), iface, 0),
        bar
    );
    assert_eq!(
        interface_method(&mut t_arena, &o_arena, &PackageArena::new(), iface, 1),
        foo
    );
}

#[test]
fn interface_embedding_merges_methods() {
    // interface I1 { Foo() }
    // interface I2 { Bar(); I1 } → I2 has methods Bar, Foo
    let (mut t_arena, _) = init_universe();
    let mut o_arena = ObjectArena::new();

    let sig = new_signature_type(&mut t_arena, None, &[], &[], None, None, false);
    let foo = new_func(&mut o_arena, "Foo", Some(sig));
    let bar = new_func(&mut o_arena, "Bar", Some(sig));

    let i1 = new_interface_type(&mut t_arena, vec![foo], vec![]);
    let i2 = new_interface_type(&mut t_arena, vec![bar], vec![i1]);

    assert_eq!(
        interface_num_methods(&mut t_arena, &o_arena, &PackageArena::new(), i2),
        2
    );
    // Sorted by name: Bar, Foo.
    assert_eq!(
        interface_method(&mut t_arena, &o_arena, &PackageArena::new(), i2, 0),
        bar
    );
    assert_eq!(
        interface_method(&mut t_arena, &o_arena, &PackageArena::new(), i2, 1),
        foo
    );
    // I2 is still purely a method set (no term restrictions).
    assert!(interface_is_method_set(
        &mut t_arena,
        &o_arena,
        &PackageArena::new(),
        i2
    ));
}

#[test]
fn interface_embedded_duplicate_method_dedupes_by_name() {
    // interface I1 { Foo() } embedded into I2 also explicitly declares Foo()
    // → still 1 method.
    let (mut t_arena, _) = init_universe();
    let mut o_arena = ObjectArena::new();

    let sig = new_signature_type(&mut t_arena, None, &[], &[], None, None, false);
    let foo_a = new_func(&mut o_arena, "Foo", Some(sig));
    let foo_b = new_func(&mut o_arena, "Foo", Some(sig));

    let i1 = new_interface_type(&mut t_arena, vec![foo_a], vec![]);
    let i2 = new_interface_type(&mut t_arena, vec![foo_b], vec![i1]);

    // Explicit-then-embedded: explicit wins (gets inserted into `seen` first).
    assert_eq!(
        interface_num_methods(&mut t_arena, &o_arena, &PackageArena::new(), i2),
        1
    );
    assert_eq!(
        interface_method(&mut t_arena, &o_arena, &PackageArena::new(), i2, 0),
        foo_b
    );
}

#[test]
fn interface_with_embedded_type_is_not_method_set() {
    // interface { int } — restricts type set to just int.
    let (mut t_arena, table) = init_universe();
    let o_arena = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let iface = new_interface_type(&mut t_arena, vec![], vec![int]);

    assert!(!interface_is_method_set(
        &mut t_arena,
        &o_arena,
        &PackageArena::new(),
        iface
    ));
    assert!(!interface_empty(
        &mut t_arena,
        &o_arena,
        &PackageArena::new(),
        iface
    ));
    assert_eq!(
        interface_num_methods(&mut t_arena, &o_arena, &PackageArena::new(), iface),
        0
    );
}

#[test]
fn interface_with_union_embed() {
    // interface { int | string }
    let (mut t_arena, table) = init_universe();
    let o_arena = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let s = table[BasicKind::String as usize];

    let t1 = new_term(false, int);
    let t2 = new_term(false, s);
    let u = new_union(&mut t_arena, vec![t1, t2]);
    let iface = new_interface_type(&mut t_arena, vec![], vec![u]);

    interface_compute_typeset(&mut t_arena, &o_arena, &PackageArena::new(), iface);
    let ts = interface_typeset(&mut t_arena, &o_arena, &PackageArena::new(), iface);

    assert_eq!(ts.num_methods(), 0);
    // The intersection of "all" and "{int, string}" is "{int, string}" — 2 terms.
    assert_eq!(ts.num_terms(), 2);
    assert!(!ts.comparable());
    // Not a method set (it has term restrictions).
    assert!(!interface_is_method_set(
        &mut t_arena,
        &o_arena,
        &PackageArena::new(),
        iface
    ));
}

#[test]
fn typeparam_iface_wraps_non_interface_bound() {
    // type T[P int] ... — P's bound is `int` (non-Interface), so iface()
    // should wrap it in an implicit interface.
    let (mut t_arena, table) = init_universe();
    let mut o_arena = ObjectArena::new();
    let int = table[BasicKind::Int as usize];

    let tn = new_type_name(&mut o_arena, "P", None);
    let tp = new_type_param(&mut t_arena, tn, Some(int));

    let iface = type_param_iface(&mut t_arena, &o_arena, &PackageArena::new(), tp);
    assert_eq!(iface.kind(&t_arena), TypeKind::Interface);
    // The wrapper has int as an embedded element → it's not a method set.
    assert!(!interface_is_method_set(
        &mut t_arena,
        &o_arena,
        &PackageArena::new(),
        iface
    ));
}

#[test]
fn typeparam_iface_passes_through_interface_bound() {
    // type T[P I] ... where I is an interface — iface() should return I
    // directly, not a wrapper.
    let (mut t_arena, _) = init_universe();
    let mut o_arena = ObjectArena::new();

    // Build a non-empty interface I to use as the bound.
    let sig = new_signature_type(&mut t_arena, None, &[], &[], None, None, false);
    let m = new_func(&mut o_arena, "Foo", Some(sig));
    let i = new_interface_type(&mut t_arena, vec![m], vec![]);

    let tn_p = new_type_name(&mut o_arena, "P", None);
    let tp = new_type_param(&mut t_arena, tn_p, Some(i));

    let resolved = type_param_iface(&mut t_arena, &o_arena, &PackageArena::new(), tp);
    assert_eq!(resolved, i, "iface() should hand back the interface as-is");
}

#[test]
fn typeparam_iface_empty_for_unset_bound() {
    let mut t_arena = init_universe().0;
    let mut o_arena = ObjectArena::new();
    let tn = new_type_name(&mut o_arena, "P", None);
    let tp = new_type_param(&mut t_arena, tn, None);

    let empty_iface = type_param_iface(&mut t_arena, &o_arena, &PackageArena::new(), tp);
    assert_eq!(empty_iface.kind(&t_arena), TypeKind::Interface);
    assert!(interface_empty(
        &mut t_arena,
        &o_arena,
        &PackageArena::new(),
        empty_iface
    ));
}

#[test]
fn signature_with_tuple_params_round_trips_through_typeset() {
    // Smoke test that the typeset code doesn't blow up when an interface
    // method's signature has tuple params.
    let (mut t_arena, table) = init_universe();
    let mut o_arena = ObjectArena::new();
    let int = table[BasicKind::Int as usize];

    let p = new_var(&mut o_arena, "x", int);
    let params = new_tuple(&mut t_arena, &[p]);
    let r = new_var(&mut o_arena, "", int);
    let results = new_tuple(&mut t_arena, &[r]);
    let sig = new_signature_type(&mut t_arena, None, &[], &[], params, results, false);

    let method = new_func(&mut o_arena, "Identity", Some(sig));
    let iface = new_interface_type(&mut t_arena, vec![method], vec![]);

    assert_eq!(
        interface_num_methods(&mut t_arena, &o_arena, &PackageArena::new(), iface),
        1
    );
    assert_eq!(
        interface_method(&mut t_arena, &o_arena, &PackageArena::new(), iface, 0),
        method
    );
}

#[test]
fn set_constraint_then_iface() {
    // Build TypeParam with no bound, set constraint later, then iface() it.
    let (mut t_arena, table) = init_universe();
    let mut o_arena = ObjectArena::new();
    let int = table[BasicKind::Int as usize];

    let tn = new_type_name(&mut o_arena, "P", None);
    let tp = new_type_param(&mut t_arena, tn, None);
    set_constraint(&mut t_arena, tp, int);

    let iface = type_param_iface(&mut t_arena, &o_arena, &PackageArena::new(), tp);
    assert_eq!(iface.kind(&t_arena), TypeKind::Interface);
    assert!(!interface_is_method_set(
        &mut t_arena,
        &o_arena,
        &PackageArena::new(),
        iface
    ));
}
