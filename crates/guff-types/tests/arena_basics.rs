//! Sanity tests for the arena foundation: ID size niche, lookup round-trip,
//! and the chunk-1 `underlying()` identity behaviour.

use std::mem::size_of;

use guff_types::{arena::TypeData, init_universe, new_pointer, BasicKind, TypeId, TypeKind};

#[test]
fn type_id_niche_optimization() {
    assert_eq!(size_of::<TypeId>(), size_of::<Option<TypeId>>());
    assert_eq!(size_of::<TypeId>(), 4);
}

#[test]
fn alloc_returns_distinct_ids() {
    let (mut arena, table) = init_universe();
    let int_id = table[BasicKind::Int as usize];
    let p1 = new_pointer(&mut arena, int_id);
    let p2 = new_pointer(&mut arena, int_id);
    assert_ne!(p1, p2, "fresh pointer allocations must have distinct IDs");
}

#[test]
fn arena_get_round_trips() {
    let (arena, table) = init_universe();
    let int_id = table[BasicKind::Int as usize];
    match arena.get(int_id) {
        TypeData::Basic(b) => assert_eq!(b.name(), "int"),
        _ => panic!("expected Basic for Int"),
    }
}

#[test]
fn underlying_is_identity_for_leaf_types() {
    let (mut arena, table) = init_universe();
    let int_id = table[BasicKind::Int as usize];
    let ptr = new_pointer(&mut arena, int_id);

    // Every chunk-1 type returns itself as its underlying type.
    for id in [int_id, ptr] {
        assert_eq!(id.underlying(&arena), id);
    }
}

#[test]
fn kind_dispatches_correctly() {
    let (mut arena, table) = init_universe();
    let int_id = table[BasicKind::Int as usize];
    assert_eq!(int_id.kind(&arena), TypeKind::Basic);

    let ptr = new_pointer(&mut arena, int_id);
    assert_eq!(ptr.kind(&arena), TypeKind::Pointer);
}
