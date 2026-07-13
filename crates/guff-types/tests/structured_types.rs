//! Construction + accessor round-trips for chunk-2 types: Struct, Signature,
//! Interface, Union. Also exercises the Func object kind.

use guff_types::{
    init_universe, interface_embedded_type, interface_explicit_method, interface_is_implicit,
    interface_mark_implicit, interface_num_embeddeds, interface_num_explicit_methods, new_func,
    new_interface_type, new_signature_type, new_slice, new_struct, new_term, new_tuple, new_union,
    new_var, signature_params, signature_recv, signature_results, signature_variadic, struct_field,
    struct_num_fields, struct_tag, union_len, union_term, BasicKind, ObjectArena, ObjectData,
    TypeKind,
};

#[test]
fn struct_round_trips_fields_and_tags() {
    let (mut t_arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];
    let str_id = table[BasicKind::String as usize];

    let mut o_arena = ObjectArena::new();
    let f1 = new_var(&mut o_arena, "x", int);
    let f2 = new_var(&mut o_arena, "y", str_id);

    // Tags shorter than fields is fine — missing entries are "".
    let s = new_struct(&mut t_arena, vec![f1, f2], vec!["json:\"x\"".to_string()]);
    assert_eq!(s.kind(&t_arena), TypeKind::Struct);
    assert_eq!(struct_num_fields(&t_arena, s), 2);
    assert_eq!(struct_field(&t_arena, s, 0), f1);
    assert_eq!(struct_field(&t_arena, s, 1), f2);
    assert_eq!(struct_tag(&t_arena, s, 0), "json:\"x\"");
    assert_eq!(struct_tag(&t_arena, s, 1), "");
}

#[test]
#[should_panic(expected = "more tags than fields")]
fn struct_panics_on_too_many_tags() {
    let mut t_arena = init_universe().0;
    new_struct(&mut t_arena, vec![], vec!["dangling".to_string()]);
}

#[test]
fn signature_round_trips() {
    let (mut t_arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let mut o_arena = ObjectArena::new();
    let p_x = new_var(&mut o_arena, "x", int);
    let r_y = new_var(&mut o_arena, "", int); // unnamed return
    let recv = new_var(&mut o_arena, "self", int);

    let params = new_tuple(&mut t_arena, &[p_x]);
    let results = new_tuple(&mut t_arena, &[r_y]);

    let sig = new_signature_type(&mut t_arena, Some(recv), &[], &[], params, results, false);
    assert_eq!(sig.kind(&t_arena), TypeKind::Signature);
    assert_eq!(signature_recv(&t_arena, sig), Some(recv));
    assert_eq!(signature_params(&t_arena, sig), params);
    assert_eq!(signature_results(&t_arena, sig), results);
    assert!(!signature_variadic(&t_arena, sig));
}

#[test]
fn signature_variadic_requires_params() {
    let mut t_arena = init_universe().0;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        new_signature_type(&mut t_arena, None, &[], &[], None, None, true);
    }));
    assert!(result.is_err(), "variadic with no params must panic");
}

#[test]
fn signature_variadic_with_slice_last_param() {
    let (mut t_arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];
    let int_slice = new_slice(&mut t_arena, int);

    let mut o_arena = ObjectArena::new();
    let p = new_var(&mut o_arena, "xs", int_slice);
    let params = new_tuple(&mut t_arena, &[p]);

    let sig = new_signature_type(&mut t_arena, None, &[], &[], params, None, true);
    assert!(signature_variadic(&t_arena, sig));
}

#[test]
fn interface_explicit_methods_and_embeddeds() {
    let (mut t_arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let mut o_arena = ObjectArena::new();
    // Build a method: signature func() int, named "Foo".
    let r = new_var(&mut o_arena, "", int);
    let results = new_tuple(&mut t_arena, &[r]);
    let sig = new_signature_type(&mut t_arena, None, &[], &[], None, results, false);
    let m = new_func(&mut o_arena, "Foo", Some(sig));

    // Stub an "embedded type" — for chunk 2 we just need any TypeId.
    let embedded = int;

    let iface = new_interface_type(&mut t_arena, vec![m], vec![embedded]);
    assert_eq!(iface.kind(&t_arena), TypeKind::Interface);
    assert_eq!(interface_num_explicit_methods(&t_arena, iface), 1);
    assert_eq!(interface_explicit_method(&t_arena, iface, 0), m);
    assert_eq!(interface_num_embeddeds(&t_arena, iface), 1);
    assert_eq!(interface_embedded_type(&t_arena, iface, 0), embedded);

    // Implicit flag starts false; mark_implicit flips it.
    assert!(!interface_is_implicit(&t_arena, iface));
    interface_mark_implicit(&mut t_arena, iface);
    assert!(interface_is_implicit(&t_arena, iface));

    // Func object accessors round-trip.
    assert_eq!(m.name(&o_arena), "Foo");
    assert_eq!(m.typ(&o_arena), Some(sig));
    match o_arena.get(m) {
        ObjectData::Func(f) => assert_eq!(f.name(), "Foo"),
        _ => panic!("expected Func variant"),
    }
}

#[test]
fn union_round_trips() {
    let (mut t_arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];
    let str_id = table[BasicKind::String as usize];

    let t1 = new_term(false, int); // `int`
    let t2 = new_term(true, str_id); // `~string`
    let u = new_union(&mut t_arena, vec![t1, t2]);

    assert_eq!(u.kind(&t_arena), TypeKind::Union);
    assert_eq!(union_len(&t_arena, u), 2);

    let term0 = union_term(&t_arena, u, 0);
    assert!(!term0.tilde());
    assert_eq!(term0.typ(), int);

    let term1 = union_term(&t_arena, u, 1);
    assert!(term1.tilde());
    assert_eq!(term1.typ(), str_id);
}

#[test]
#[should_panic(expected = "empty union")]
fn empty_union_panics() {
    let mut t_arena = init_universe().0;
    new_union(&mut t_arena, vec![]);
}

#[test]
fn func_two_phase_construction() {
    let mut o_arena = ObjectArena::new();
    // Build a Func with no signature yet — matches Go's NewFunc(.., nil).
    let f = new_func(&mut o_arena, "Lazy", None);
    assert_eq!(f.name(&o_arena), "Lazy");
    assert_eq!(f.typ(&o_arena), None);
}
