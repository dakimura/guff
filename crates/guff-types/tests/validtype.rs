//! Chunk-10 tests: `validtype.rs` — recursive-type validity.

use guff_types::{
    init_universe_full, make_obj_list, named_underlying, new_array, new_field, new_named,
    new_pointer, new_struct, new_type_name, set_underlying, valid_type, BasicKind, TypeData,
    ValidResult,
};

#[test]
fn simple_named_struct_is_valid() {
    // type T struct { x int }
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let invalid = u.typ[BasicKind::Invalid as usize];

    let tn = new_type_name(&mut u.object_arena, "T", None);
    let x = new_field(&mut u.object_arena, "x", int, false);
    let s = new_struct(&mut u.type_arena, vec![x], vec![String::new()]);
    let named = new_named(&mut u.type_arena, &mut u.object_arena, tn, Some(s), vec![]);

    let result = valid_type(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        named,
        invalid,
    );
    assert!(result.is_valid(), "expected Valid, got {:?}", result);
}

#[test]
fn pointer_to_self_named_is_valid() {
    // type T struct { next *T }  — a pointer breaks the layout chain, so
    // T is valid.
    let mut u = init_universe_full();
    let invalid = u.typ[BasicKind::Invalid as usize];

    let tn = new_type_name(&mut u.object_arena, "T", None);
    let named_t = new_named(&mut u.type_arena, &mut u.object_arena, tn, None, vec![]);
    let ptr_t = new_pointer(&mut u.type_arena, named_t);
    let field_next = new_field(&mut u.object_arena, "next", ptr_t, false);
    let s = new_struct(&mut u.type_arena, vec![field_next], vec![String::new()]);
    set_underlying(&mut u.type_arena, named_t, s);

    let result = valid_type(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        named_t,
        invalid,
    );
    assert!(result.is_valid(), "expected Valid, got {:?}", result);
}

#[test]
fn struct_directly_containing_self_is_invalid() {
    // type T struct { t T }  — direct self-containment ⇒ cycle.
    let mut u = init_universe_full();
    let invalid = u.typ[BasicKind::Invalid as usize];

    let tn = new_type_name(&mut u.object_arena, "T", None);
    let named_t = new_named(&mut u.type_arena, &mut u.object_arena, tn, None, vec![]);
    let field_t = new_field(&mut u.object_arena, "t", named_t, false);
    let s = new_struct(&mut u.type_arena, vec![field_t], vec![String::new()]);
    set_underlying(&mut u.type_arena, named_t, s);

    let result = valid_type(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        named_t,
        invalid,
    );
    match &result {
        ValidResult::Cycle { path } => {
            assert_eq!(path.len(), 1);
            assert_eq!(path[0], tn);
        }
        ValidResult::Valid => panic!("expected Cycle, got Valid"),
    }

    // Origin's underlying should now be Invalid.
    let u_typ = named_underlying(&u.type_arena, named_t).expect("underlying set");
    match u.type_arena.get(u_typ) {
        TypeData::Basic(b) => assert_eq!(b.kind(), BasicKind::Invalid),
        _ => panic!("expected Invalid Basic"),
    }
}

#[test]
fn array_of_self_is_invalid() {
    // type T [10]T  — fixed-size array of self ⇒ cycle.
    let mut u = init_universe_full();
    let invalid = u.typ[BasicKind::Invalid as usize];

    let tn = new_type_name(&mut u.object_arena, "T", None);
    let named_t = new_named(&mut u.type_arena, &mut u.object_arena, tn, None, vec![]);
    let arr = new_array(&mut u.type_arena, named_t, 10);
    set_underlying(&mut u.type_arena, named_t, arr);

    let result = valid_type(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        named_t,
        invalid,
    );
    matches!(result, ValidResult::Cycle { .. });
}

#[test]
fn mutual_recursion_through_struct_fields_is_invalid() {
    // type A struct { b B }
    // type B struct { a A }   — A contains B contains A ⇒ cycle.
    let mut u = init_universe_full();
    let invalid = u.typ[BasicKind::Invalid as usize];

    let tn_a = new_type_name(&mut u.object_arena, "A", None);
    let tn_b = new_type_name(&mut u.object_arena, "B", None);
    let named_a = new_named(&mut u.type_arena, &mut u.object_arena, tn_a, None, vec![]);
    let named_b = new_named(&mut u.type_arena, &mut u.object_arena, tn_b, None, vec![]);

    let fb = new_field(&mut u.object_arena, "b", named_b, false);
    let s_a = new_struct(&mut u.type_arena, vec![fb], vec![String::new()]);
    set_underlying(&mut u.type_arena, named_a, s_a);

    let fa = new_field(&mut u.object_arena, "a", named_a, false);
    let s_b = new_struct(&mut u.type_arena, vec![fa], vec![String::new()]);
    set_underlying(&mut u.type_arena, named_b, s_b);

    let result = valid_type(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        named_a,
        invalid,
    );
    match result {
        ValidResult::Cycle { path } => {
            // Path should contain both A and B; the cycle starts at A.
            assert!(path.contains(&tn_a));
            assert!(path.contains(&tn_b));
        }
        ValidResult::Valid => panic!("expected Cycle"),
    }
}

#[test]
fn make_obj_list_returns_typename_objects() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let tn1 = new_type_name(&mut u.object_arena, "A", None);
    let n1 = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn1,
        Some(int),
        vec![],
    );
    let tn2 = new_type_name(&mut u.object_arena, "B", None);
    let n2 = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn2,
        Some(int),
        vec![],
    );

    let objs = make_obj_list(&u.type_arena, &[n1, n2]);
    assert_eq!(objs, vec![tn1, tn2]);
}
