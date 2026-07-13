//! S01: core id + arena + Value handle tests.

use guff_ssa::arena::Arena;
use guff_ssa::ids::{BlockId, FuncId, InstrId};
use guff_ssa::{ArenaId, Value};

#[test]
fn alloc_returns_stable_sequential_ids() {
    let mut a: Arena<FuncId, &'static str> = Arena::new();
    assert!(a.is_empty());
    let x = a.alloc("f0");
    let y = a.alloc("f1");
    let z = a.alloc("f2");
    assert_eq!(a.len(), 3);
    assert_eq!(*a.get(x), "f0");
    assert_eq!(*a.get(y), "f1");
    assert_eq!(*a.get(z), "f2");
    // Distinct allocations yield distinct handles.
    assert_ne!(x, y);
    assert_ne!(y, z);
}

#[test]
fn get_mut_round_trips() {
    let mut a: Arena<BlockId, i32> = Arena::new();
    let id = a.alloc(41);
    *a.get_mut(id) += 1;
    assert_eq!(*a.get(id), 42);
}

#[test]
fn iter_yields_handles_in_insertion_order() {
    let mut a: Arena<InstrId, char> = Arena::new();
    let ids: Vec<_> = ['a', 'b', 'c'].into_iter().map(|c| a.alloc(c)).collect();
    let collected: Vec<_> = a.iter().collect();
    assert_eq!(collected.len(), 3);
    for (i, (id, &val)) in collected.iter().enumerate() {
        assert_eq!(*id, ids[i]);
        assert_eq!(val, ['a', 'b', 'c'][i]);
    }
}

#[test]
fn from_index_and_index_are_inverse() {
    for i in [0usize, 1, 2, 100, 65_535] {
        assert_eq!(FuncId::from_index(i).index(), i);
    }
}

#[test]
fn option_id_uses_niche_and_stays_four_bytes() {
    // NonZeroU32 niche: Option<Id> must not grow past a bare id.
    assert_eq!(std::mem::size_of::<FuncId>(), 4);
    assert_eq!(std::mem::size_of::<Option<FuncId>>(), 4);
    assert_eq!(std::mem::size_of::<Option<InstrId>>(), 4);
}

#[test]
fn value_is_copy_and_compact() {
    // A Value is a tag plus a NonZeroU32 payload.
    assert_eq!(std::mem::size_of::<Value>(), 8);
    let mut a: Arena<InstrId, ()> = Arena::new();
    let id = a.alloc(());
    let v = Value::Instr(id);
    let w = v; // Copy, not move.
    assert_eq!(v, w);
    assert!(matches!(w, Value::Instr(_)));
}
