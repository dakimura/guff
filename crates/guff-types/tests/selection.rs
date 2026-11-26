//! Chunk-10 tests: `selection.rs` — Selection struct + `selection_type`.

use guff_types::{
    init_universe_full, new_field, new_func, new_param, new_signature_type, new_struct, new_tuple,
    new_type_name, selection_string, selection_type, signature_params, signature_recv,
    signature_results, signature_variadic, tuple_at, tuple_len, BasicKind, Selection,
    SelectionKind,
};

#[test]
fn field_val_selection_returns_field_type() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    // struct { X int }
    let field_x = new_field(&mut u.object_arena, "X", int, false);
    let s = new_struct(&mut u.type_arena, vec![field_x], vec![String::new()]);

    let sel = Selection::new(SelectionKind::FieldVal, s, field_x, vec![0], false);
    let ty = selection_type(&mut u.type_arena, &mut u.object_arena, &sel);
    assert_eq!(ty, int);
    assert_eq!(sel.index(), &[0]);
    assert!(!sel.indirect());
}

#[test]
fn method_val_selection_replaces_receiver_type() {
    // type T struct{}; func (recv *origT) m() string  — for selection use
    // the original receiver type but verify selection.Type() patches it
    // with `sel.recv`.
    let mut u = init_universe_full();
    let s = u.typ[BasicKind::String as usize];

    // Original receiver type — distinct from sel.recv.
    let orig_recv_tn = new_type_name(&mut u.object_arena, "origT", None);
    let orig_recv_named = guff_types::new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        orig_recv_tn,
        Some(u.typ[BasicKind::Int as usize]),
        vec![],
    );
    let recv_var = new_param(&mut u.object_arena, "r", orig_recv_named);
    let result_var = new_param(&mut u.object_arena, "", s);
    let results = new_tuple(&mut u.type_arena, &[result_var]);
    // func (r origT) m() string
    let sig = new_signature_type(
        &mut u.type_arena,
        Some(recv_var),
        &[],
        &[],
        None,
        results,
        false,
    );
    let m = new_func(&mut u.object_arena, "m", Some(sig));

    // sel.recv is a *different* type — say plain int — to verify that
    // selection_type's new Signature uses sel.recv, not origT.
    let sel_recv = u.typ[BasicKind::Int as usize];
    let sel = Selection::new(SelectionKind::MethodVal, sel_recv, m, vec![0], false);

    let new_ty = selection_type(&mut u.type_arena, &mut u.object_arena, &sel);
    // Must be a Signature with recv.typ == sel.recv.
    let new_recv_obj = signature_recv(&u.type_arena, new_ty).expect("MethodVal sig has recv");
    let new_recv_typ = new_recv_obj.typ(&u.object_arena).unwrap();
    assert_eq!(new_recv_typ, sel_recv);
    // Original sig untouched.
    assert_eq!(
        recv_var.typ(&u.object_arena),
        Some(orig_recv_named),
        "original recv var must remain pointing at origT"
    );
    // Results carried over.
    let results = signature_results(&u.type_arena, new_ty).unwrap();
    assert_eq!(tuple_len(&u.type_arena, Some(results)), 1);
    let r0 = tuple_at(&u.type_arena, results, 0);
    assert_eq!(r0.typ(&u.object_arena), Some(s));
    assert!(!signature_variadic(&u.type_arena, new_ty));
}

#[test]
fn method_expr_selection_promotes_receiver_to_first_param() {
    // func (r origT) m(x int) — MethodExpr ⇒ func(t sel.recv, x int)
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];

    let orig_recv_tn = new_type_name(&mut u.object_arena, "origT", None);
    let orig_recv = guff_types::new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        orig_recv_tn,
        Some(int),
        vec![],
    );
    let recv_var = new_param(&mut u.object_arena, "r", orig_recv);
    let x = new_param(&mut u.object_arena, "x", int);
    let params = new_tuple(&mut u.type_arena, &[x]);
    let sig = new_signature_type(
        &mut u.type_arena,
        Some(recv_var),
        &[],
        &[],
        params,
        None,
        false,
    );
    let m = new_func(&mut u.object_arena, "m", Some(sig));

    let s = u.typ[BasicKind::String as usize];
    // sel.recv is `string` — verifies the promoted first param picks up
    // sel.recv (not origT).
    let sel = Selection::new(SelectionKind::MethodExpr, s, m, vec![0], false);

    let new_ty = selection_type(&mut u.type_arena, &mut u.object_arena, &sel);
    // No receiver on the result sig.
    assert!(signature_recv(&u.type_arena, new_ty).is_none());
    // Params: [sel.recv, int].
    let new_params = signature_params(&u.type_arena, new_ty).expect("non-empty params");
    assert_eq!(tuple_len(&u.type_arena, Some(new_params)), 2);
    let p0 = tuple_at(&u.type_arena, new_params, 0);
    let p1 = tuple_at(&u.type_arena, new_params, 1);
    assert_eq!(p0.typ(&u.object_arena), Some(s));
    assert_eq!(p1.typ(&u.object_arena), Some(int));
    // Original `x` param object is reused (Go does the same: appends
    // existing params to the new tuple).
    assert_eq!(p1, x);
}

#[test]
fn selection_string_prefix_matches_kind() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let field = new_field(&mut u.object_arena, "F", int, false);

    let s_field = Selection::new(SelectionKind::FieldVal, int, field, vec![0], false);
    let s_mv = Selection::new(SelectionKind::MethodVal, int, field, vec![0], false);
    let s_me = Selection::new(SelectionKind::MethodExpr, int, field, vec![0], false);

    assert!(
        selection_string(&u.type_arena, &u.object_arena, &u.package_arena, &s_field)
            .starts_with("field")
    );
    assert!(
        selection_string(&u.type_arena, &u.object_arena, &u.package_arena, &s_mv)
            .starts_with("method ")
    );
    assert!(
        selection_string(&u.type_arena, &u.object_arena, &u.package_arena, &s_me)
            .starts_with("method expr")
    );
}

#[test]
fn selection_accessors_round_trip() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let field = new_field(&mut u.object_arena, "F", int, false);

    let sel = Selection::new(SelectionKind::FieldVal, int, field, vec![1, 2, 3], true);
    assert_eq!(sel.kind(), SelectionKind::FieldVal);
    assert_eq!(sel.recv(), int);
    assert_eq!(sel.obj(), field);
    assert_eq!(sel.index(), &[1, 2, 3]);
    assert!(sel.indirect());
}
