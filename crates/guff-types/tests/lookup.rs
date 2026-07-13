//! Chunk-11 tests: `lookup.rs` — field & method resolution through
//! embedded types, with the various corner cases (collisions, indirection,
//! pointer-receiver gating, interface methods).

use guff_types::{
    add_method, as_named, concat, deref, deref_struct_ptr, field_index,
    has_invalid_embedded_fields, init_universe_full, is_interface_ptr, lookup_field_or_method,
    lookup_field_or_method_fold, lookup_selection, method_index, new_field, new_func,
    new_interface_type, new_named, new_param, new_pointer, new_signature_type, new_struct,
    new_type_name, signature_recv, LookupResult, ObjectData, SelectionKind, TypeData,
};

// ----------------------------------------------------------------------------
// Small helpers

fn make_field_struct(
    type_arena: &mut guff_types::TypeArena,
    object_arena: &mut guff_types::ObjectArena,
    fields: Vec<(&str, guff_types::TypeId)>,
) -> guff_types::TypeId {
    let n = fields.len();
    let mut field_objs = Vec::with_capacity(n);
    let mut tags = Vec::new();
    for (name, ty) in fields {
        field_objs.push(new_field(object_arena, name, ty, false));
        tags.push(String::new());
    }
    new_struct(type_arena, field_objs, tags)
}

// ----------------------------------------------------------------------------
// Field lookup

#[test]
fn finds_field_at_depth_zero() {
    let mut u = init_universe_full();
    let int = u.typ[guff_types::BasicKind::Int as usize];
    let s = make_field_struct(&mut u.type_arena, &mut u.object_arena, vec![("X", int)]);

    let res = lookup_field_or_method(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        s,
        false,
        None,
        "X",
    );
    let (obj, index, indirect) = res.found().expect("X is found");
    assert_eq!(index, &[0]);
    assert!(!indirect);
    assert_eq!(obj.name(&u.object_arena), "X");
}

#[test]
fn blank_name_is_never_found() {
    let mut u = init_universe_full();
    let int = u.typ[guff_types::BasicKind::Int as usize];
    let s = make_field_struct(&mut u.type_arena, &mut u.object_arena, vec![("_", int)]);
    let res = lookup_field_or_method(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        s,
        false,
        None,
        "_",
    );
    assert!(matches!(res, LookupResult::NotFound));
}

#[test]
fn finds_field_through_embedded_struct() {
    // type Inner struct { X int }
    // type Outer struct { Inner }      // embedded
    // Outer{}.X  →  found at index [0, 0]
    let mut u = init_universe_full();
    let int = u.typ[guff_types::BasicKind::Int as usize];
    let inner = make_field_struct(&mut u.type_arena, &mut u.object_arena, vec![("X", int)]);
    let tn_inner = new_type_name(&mut u.object_arena, "Inner", None);
    let named_inner = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn_inner,
        Some(inner),
        vec![],
    );

    // Embedded field "Inner" with type Named(Inner).
    let embedded = new_field(&mut u.object_arena, "Inner", named_inner, true);
    let outer = new_struct(&mut u.type_arena, vec![embedded], vec![String::new()]);

    let res = lookup_field_or_method(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        outer,
        false,
        None,
        "X",
    );
    let (obj, index, indirect) = res.found().expect("X found through embedded Inner");
    assert_eq!(index, &[0, 0]);
    assert!(!indirect);
    assert_eq!(obj.name(&u.object_arena), "X");
}

#[test]
fn finds_field_through_embedded_pointer_marks_indirect() {
    // type Outer struct { *Inner } — selecting X requires going through a
    // pointer, so indirect=true.
    let mut u = init_universe_full();
    let int = u.typ[guff_types::BasicKind::Int as usize];
    let inner = make_field_struct(&mut u.type_arena, &mut u.object_arena, vec![("X", int)]);
    let tn_inner = new_type_name(&mut u.object_arena, "Inner", None);
    let named_inner = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn_inner,
        Some(inner),
        vec![],
    );
    let ptr_inner = new_pointer(&mut u.type_arena, named_inner);
    let embedded = new_field(&mut u.object_arena, "Inner", ptr_inner, true);
    let outer = new_struct(&mut u.type_arena, vec![embedded], vec![String::new()]);

    let res = lookup_field_or_method(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        outer,
        false,
        None,
        "X",
    );
    let (_, _, indirect) = res.found().expect("X found");
    assert!(indirect, "indirection through *Inner must be flagged");
}

#[test]
fn ambiguous_collision_at_same_depth_returns_ambiguous() {
    // type A struct { X int }
    // type B struct { X int }
    // type C struct { A; B }   →  C{}.X is ambiguous.
    let mut u = init_universe_full();
    let int = u.typ[guff_types::BasicKind::Int as usize];
    let s_a = make_field_struct(&mut u.type_arena, &mut u.object_arena, vec![("X", int)]);
    let s_b = make_field_struct(&mut u.type_arena, &mut u.object_arena, vec![("X", int)]);
    let tn_a = new_type_name(&mut u.object_arena, "A", None);
    let na = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn_a,
        Some(s_a),
        vec![],
    );
    let tn_b = new_type_name(&mut u.object_arena, "B", None);
    let nb = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn_b,
        Some(s_b),
        vec![],
    );

    let fa = new_field(&mut u.object_arena, "A", na, true);
    let fb = new_field(&mut u.object_arena, "B", nb, true);
    let outer = new_struct(
        &mut u.type_arena,
        vec![fa, fb],
        vec![String::new(), String::new()],
    );

    let res = lookup_field_or_method(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        outer,
        false,
        None,
        "X",
    );
    assert!(matches!(res, LookupResult::Ambiguous { .. }));
}

// ----------------------------------------------------------------------------
// Method lookup

#[test]
fn finds_method_on_named_type() {
    // type T struct{}; func (T) M()
    let mut u = init_universe_full();
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
    let m = new_func(&mut u.object_arena, "M", Some(sig));
    add_method(&mut u.type_arena, &u.object_arena, t, m);

    let res = lookup_field_or_method(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        t,
        false,
        None,
        "M",
    );
    let (obj, index, indirect) = res.found().expect("M found");
    assert_eq!(obj, m);
    assert_eq!(index, &[0]);
    assert!(!indirect);
}

#[test]
fn pointer_receiver_method_requires_addressable_or_indirect() {
    // type T struct{}; func (*T) M()
    // Selecting M on a non-addressable value-of-T must return PtrRecvRequired.
    let mut u = init_universe_full();
    let empty = new_struct(&mut u.type_arena, vec![], vec![]);
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let t = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn,
        Some(empty),
        vec![],
    );
    let ptr_t = new_pointer(&mut u.type_arena, t);
    let recv = new_param(&mut u.object_arena, "r", ptr_t);
    let sig = new_signature_type(&mut u.type_arena, Some(recv), &[], &[], None, None, false);
    let m = new_func(&mut u.object_arena, "M", Some(sig));
    add_method(&mut u.type_arena, &u.object_arena, t, m);

    // Non-addressable selection: should produce PtrRecvRequired.
    let res = lookup_field_or_method(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        t,
        false,
        None,
        "M",
    );
    assert!(
        matches!(res, LookupResult::PtrRecvRequired),
        "got: {:?}",
        res
    );

    // Addressable: found.
    let res2 = lookup_field_or_method(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        t,
        true,
        None,
        "M",
    );
    assert!(res2.found().is_some());

    // Through a pointer-of-T: found regardless of addressability (indirect).
    let res3 = lookup_field_or_method(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        ptr_t,
        false,
        None,
        "M",
    );
    let (_, _, indirect) = res3.found().expect("found via *T");
    assert!(indirect);
}

#[test]
fn finds_method_on_interface_typeset() {
    // type I interface { M() }
    let mut u = init_universe_full();
    // Build the method with a placeholder signature.
    let sig = new_signature_type(&mut u.type_arena, None, &[], &[], None, None, false);
    let m = new_func(&mut u.object_arena, "M", Some(sig));
    let iface = new_interface_type(&mut u.type_arena, vec![m], vec![]);

    let res = lookup_field_or_method(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        iface,
        false,
        None,
        "M",
    );
    let (obj, index, _) = res.found().expect("M found on interface");
    assert_eq!(obj, m);
    assert_eq!(index, &[0]);
}

#[test]
fn fold_case_finds_differently_cased_name() {
    let mut u = init_universe_full();
    let int = u.typ[guff_types::BasicKind::Int as usize];
    let s = make_field_struct(&mut u.type_arena, &mut u.object_arena, vec![("Xfoo", int)]);
    // Without fold_case → miss.
    let r1 = lookup_field_or_method(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        s,
        false,
        None,
        "xfoo",
    );
    assert!(matches!(r1, LookupResult::NotFound));
    // With fold_case → hit.
    let r2 = lookup_field_or_method_fold(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        s,
        false,
        None,
        "xfoo",
        true,
    );
    assert!(r2.found().is_some());
}

#[test]
fn lookup_selection_wraps_into_selection() {
    let mut u = init_universe_full();
    let int = u.typ[guff_types::BasicKind::Int as usize];
    let s = make_field_struct(&mut u.type_arena, &mut u.object_arena, vec![("X", int)]);

    let sel = lookup_selection(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        s,
        false,
        None,
        "X",
    )
    .expect("selection produced");
    assert_eq!(sel.kind(), SelectionKind::FieldVal);
    assert_eq!(sel.recv(), s);
    assert_eq!(sel.index(), &[0]);
}

// ----------------------------------------------------------------------------
// Utility helpers

#[test]
fn deref_handles_pointer_and_not_named_pointer() {
    let mut u = init_universe_full();
    let int = u.typ[guff_types::BasicKind::Int as usize];
    let ptr_int = new_pointer(&mut u.type_arena, int);
    assert_eq!(deref(&u.type_arena, ptr_int), (int, true));
    assert_eq!(deref(&u.type_arena, int), (int, false));

    // Named pointer: type T *int — deref returns the Named itself + false.
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let named_ptr = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn,
        Some(ptr_int),
        vec![],
    );
    assert_eq!(deref(&u.type_arena, named_ptr), (named_ptr, false));
}

#[test]
fn deref_struct_ptr_unwraps_pointer_to_struct() {
    let mut u = init_universe_full();
    let int = u.typ[guff_types::BasicKind::Int as usize];
    let s = make_field_struct(&mut u.type_arena, &mut u.object_arena, vec![("X", int)]);
    let p = new_pointer(&mut u.type_arena, s);
    assert_eq!(deref_struct_ptr(&u.type_arena, p), s);
    assert_eq!(deref_struct_ptr(&u.type_arena, s), s);
}

#[test]
fn is_interface_ptr_detects_pointer_to_interface() {
    let mut u = init_universe_full();
    let iface = new_interface_type(&mut u.type_arena, vec![], vec![]);
    let p = new_pointer(&mut u.type_arena, iface);
    assert!(is_interface_ptr(&u.type_arena, p));
    let int = u.typ[guff_types::BasicKind::Int as usize];
    let np = new_pointer(&mut u.type_arena, int);
    assert!(!is_interface_ptr(&u.type_arena, np));
}

#[test]
fn concat_field_method_index_basics() {
    let mut u = init_universe_full();
    let int = u.typ[guff_types::BasicKind::Int as usize];

    assert_eq!(concat(&[1, 2], 3), vec![1, 2, 3]);

    let fa = new_field(&mut u.object_arena, "A", int, false);
    let fb = new_field(&mut u.object_arena, "B", int, false);
    assert_eq!(
        field_index(
            &u.object_arena,
            &u.package_arena,
            &[fa, fb],
            None,
            "B",
            false
        ),
        Some(1)
    );
    assert_eq!(
        field_index(
            &u.object_arena,
            &u.package_arena,
            &[fa, fb],
            None,
            "C",
            false
        ),
        None
    );

    let sig = new_signature_type(&mut u.type_arena, None, &[], &[], None, None, false);
    let m1 = new_func(&mut u.object_arena, "M", Some(sig));
    let m2 = new_func(&mut u.object_arena, "N", Some(sig));
    assert_eq!(
        method_index(
            &u.object_arena,
            &u.package_arena,
            &[m1, m2],
            None,
            "N",
            false
        )
        .map(|(i, _)| i),
        Some(1)
    );
}

#[test]
fn has_invalid_embedded_fields_handles_invalid_basic() {
    // type S struct { Bad invalid }  → has_invalid_embedded_fields = true
    let mut u = init_universe_full();
    let invalid = u.typ[guff_types::BasicKind::Invalid as usize];
    let bad = new_field(&mut u.object_arena, "Bad", invalid, true);
    let s = new_struct(&mut u.type_arena, vec![bad], vec![String::new()]);
    assert!(has_invalid_embedded_fields(
        &u.type_arena,
        &u.object_arena,
        s
    ));

    // No embedded invalid → false.
    let int = u.typ[guff_types::BasicKind::Int as usize];
    let good = new_field(&mut u.object_arena, "Good", int, false);
    let s2 = new_struct(&mut u.type_arena, vec![good], vec![String::new()]);
    assert!(!has_invalid_embedded_fields(
        &u.type_arena,
        &u.object_arena,
        s2
    ));
}

#[test]
fn as_named_returns_underlying_named() {
    let mut u = init_universe_full();
    let int = u.typ[guff_types::BasicKind::Int as usize];
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let t = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn,
        Some(int),
        vec![],
    );
    assert_eq!(as_named(&u.type_arena, t), Some(t));
    assert_eq!(as_named(&u.type_arena, int), None);

    // Silence imports.
    let _ = signature_recv;
    let _: Option<ObjectData> = None.map(|_: ()| panic!());
    let _ = TypeData::Basic;
}
