//! Verify Tuple's "nil = empty" semantics and indexing round-trip.

use guff_types::{
    init_universe, new_tuple, new_var, tuple_at, tuple_len, BasicKind, ObjectArena, TypeKind,
};

#[test]
fn empty_tuple_is_none() {
    let mut arena = init_universe().0;
    let id = new_tuple(&mut arena, &[]);
    assert!(
        id.is_none(),
        "empty tuple should be None (matches Go's nil *Tuple)"
    );

    // tuple_len(None) == 0 — also matches Go's (*Tuple).Len() on nil receiver.
    assert_eq!(tuple_len(&arena, None), 0);
}

#[test]
fn non_empty_tuple_round_trips() {
    let (mut t_arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];
    let str_id = table[BasicKind::String as usize];

    let mut o_arena = ObjectArena::new();
    let v1 = new_var(&mut o_arena, "x", int);
    let v2 = new_var(&mut o_arena, "y", str_id);

    let tup = new_tuple(&mut t_arena, &[v1, v2]).expect("non-empty tuple should allocate");
    assert_eq!(tup.kind(&t_arena), TypeKind::Tuple);
    assert_eq!(tuple_len(&t_arena, Some(tup)), 2);
    assert_eq!(tuple_at(&t_arena, tup, 0), v1);
    assert_eq!(tuple_at(&t_arena, tup, 1), v2);

    // The Var objects retain their names and types via ObjectId accessors.
    // typ() is Option<TypeId> because Func objects support two-phase
    // construction; for Var, the typ is always set.
    assert_eq!(v1.name(&o_arena), "x");
    assert_eq!(v1.typ(&o_arena), Some(int));
    assert_eq!(v2.name(&o_arena), "y");
    assert_eq!(v2.typ(&o_arena), Some(str_id));
}
