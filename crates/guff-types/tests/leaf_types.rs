//! Construct each chunk-1 leaf type and verify every accessor round-trips.

use guff_types::{
    array_elem, array_len, basic_info, basic_kind, basic_name, chan_dir, chan_elem, init_universe,
    map_elem, map_key, new_array, new_chan, new_map, new_pointer, new_slice, pointer_elem,
    slice_elem, BasicKind, ChanDir, TypeKind, BYTE, IS_INTEGER, IS_UNSIGNED, IS_UNTYPED, RUNE,
};

#[test]
fn predeclared_basic_table_is_complete() {
    let (arena, table) = init_universe();

    // Spot-check each kind's identity in the table.
    let int = table[BasicKind::Int as usize];
    assert_eq!(basic_kind(&arena, int), BasicKind::Int);
    assert_eq!(basic_name(&arena, int), "int");
    assert!(basic_info(&arena, int).contains(IS_INTEGER));

    let s = table[BasicKind::String as usize];
    assert_eq!(basic_name(&arena, s), "string");

    let uintptr = table[BasicKind::Uintptr as usize];
    assert!(basic_info(&arena, uintptr).contains(IS_INTEGER | IS_UNSIGNED));

    let untyped_nil = table[BasicKind::UntypedNil as usize];
    assert!(basic_info(&arena, untyped_nil).contains(IS_UNTYPED));
    assert_eq!(basic_name(&arena, untyped_nil), "untyped nil");

    // unsafe.Pointer uses the bare name "Pointer" (no flags set).
    let unsafe_ptr = table[BasicKind::UnsafePointer as usize];
    assert_eq!(basic_name(&arena, unsafe_ptr), "Pointer");
}

#[test]
fn byte_and_rune_aliases_resolve_to_uint8_and_int32() {
    let (arena, table) = init_universe();
    // BYTE / RUNE are BasicKind aliases — table[BYTE as usize] resolves via
    // the uint8 / int32 entries.
    assert_eq!(basic_name(&arena, table[BYTE as usize]), "uint8");
    assert_eq!(basic_name(&arena, table[RUNE as usize]), "int32");
}

#[test]
fn array_accessors() {
    let (mut arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];
    let a = new_array(&mut arena, int, 7);
    assert_eq!(a.kind(&arena), TypeKind::Array);
    assert_eq!(array_len(&arena, a), 7);
    assert_eq!(array_elem(&arena, a), int);

    // Negative length is allowed (Go convention for unknown length).
    let unknown = new_array(&mut arena, int, -1);
    assert_eq!(array_len(&arena, unknown), -1);
}

#[test]
fn slice_accessors() {
    let (mut arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];
    let s = new_slice(&mut arena, int);
    assert_eq!(s.kind(&arena), TypeKind::Slice);
    assert_eq!(slice_elem(&arena, s), int);
}

#[test]
fn slice_elem_resolves_named() {
    // Named slice types (e.g. `type Bytes []byte`) must not panic in slice_elem.
    use guff_types::{new_named, new_type_name, ObjectArena};
    let (mut arena, table) = init_universe();
    let mut objects = ObjectArena::new();
    let byte = table[BYTE as usize];
    let slice = new_slice(&mut arena, byte);
    let tn = new_type_name(&mut objects, "Bytes", None);
    let named = new_named(&mut arena, &mut objects, tn, Some(slice), vec![]);
    assert_eq!(slice_elem(&arena, named), byte);
}

#[test]
fn pointer_accessors() {
    let (mut arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];
    let p = new_pointer(&mut arena, int);
    assert_eq!(p.kind(&arena), TypeKind::Pointer);
    assert_eq!(pointer_elem(&arena, p), int);

    // Pointer to pointer to int.
    let pp = new_pointer(&mut arena, p);
    assert_eq!(pointer_elem(&arena, pp), p);
}

#[test]
fn map_accessors() {
    let (mut arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];
    let str_id = table[BasicKind::String as usize];
    let m = new_map(&mut arena, str_id, int);
    assert_eq!(m.kind(&arena), TypeKind::Map);
    assert_eq!(map_key(&arena, m), str_id);
    assert_eq!(map_elem(&arena, m), int);
}

#[test]
fn chan_accessors() {
    let (mut arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    for dir in [ChanDir::SendRecv, ChanDir::SendOnly, ChanDir::RecvOnly] {
        let c = new_chan(&mut arena, dir, int);
        assert_eq!(c.kind(&arena), TypeKind::Chan);
        assert_eq!(chan_dir(&arena, c), dir);
        assert_eq!(chan_elem(&arena, c), int);
    }
}
