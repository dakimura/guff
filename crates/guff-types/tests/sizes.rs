//! Chunk-45 tests: `sizes.rs` — `Sizes` (StdSizes/gcSizes), `align`,
//! `sizes_for`/`default_sizes`.

use guff_types::{
    align, default_sizes, init_universe_full, new_array, new_field, new_pointer, new_slice,
    new_struct, sizes_for, BasicKind, Sizes, SizesKind,
};

/// Helper: size of a basic type under the default (gc amd64 {8,8}) sizes.
fn basic_sizeof(kind: BasicKind) -> i64 {
    let u = init_universe_full();
    let s = default_sizes();
    let t = u.typ[kind as usize];
    s.sizeof(&u.type_arena, &u.object_arena, &u.package_arena, t)
}

fn basic_alignof(kind: BasicKind) -> i64 {
    let u = init_universe_full();
    let s = default_sizes();
    let t = u.typ[kind as usize];
    s.alignof(&u.type_arena, &u.object_arena, &u.package_arena, t)
}

#[test]
fn default_sizes_is_gc_amd64() {
    let s = default_sizes();
    assert_eq!(s.kind, SizesKind::Gc);
    assert_eq!(s.word_size, 8);
    assert_eq!(s.max_align, 8);
}

#[test]
fn sizeof_explicitly_sized_basics() {
    assert_eq!(basic_sizeof(BasicKind::Bool), 1);
    assert_eq!(basic_sizeof(BasicKind::Int8), 1);
    assert_eq!(basic_sizeof(BasicKind::Int16), 2);
    assert_eq!(basic_sizeof(BasicKind::Int32), 4);
    assert_eq!(basic_sizeof(BasicKind::Int64), 8);
    assert_eq!(basic_sizeof(BasicKind::Uint16), 2);
    assert_eq!(basic_sizeof(BasicKind::Float32), 4);
    assert_eq!(basic_sizeof(BasicKind::Float64), 8);
    assert_eq!(basic_sizeof(BasicKind::Complex64), 8);
    assert_eq!(basic_sizeof(BasicKind::Complex128), 16);
}

#[test]
fn sizeof_word_sized_basics() {
    // int/uint/uintptr/unsafe.Pointer are not in basicSizes -> word size.
    assert_eq!(basic_sizeof(BasicKind::Int), 8);
    assert_eq!(basic_sizeof(BasicKind::Uint), 8);
    assert_eq!(basic_sizeof(BasicKind::Uintptr), 8);
    assert_eq!(basic_sizeof(BasicKind::UnsafePointer), 8);
    // string is 2 * word size.
    assert_eq!(basic_sizeof(BasicKind::String), 16);
}

#[test]
fn alignof_basics() {
    assert_eq!(basic_alignof(BasicKind::Bool), 1);
    assert_eq!(basic_alignof(BasicKind::Int8), 1);
    assert_eq!(basic_alignof(BasicKind::Int16), 2);
    assert_eq!(basic_alignof(BasicKind::Int64), 8);
    // complex are aligned like [2]float: align = size/2.
    assert_eq!(basic_alignof(BasicKind::Complex64), 4); // size 8 -> 4
    assert_eq!(basic_alignof(BasicKind::Complex128), 8); // size 16 -> 8 (== maxAlign)
                                                         // string aligns to word size.
    assert_eq!(basic_alignof(BasicKind::String), 8);
}

#[test]
fn slice_and_pointer() {
    let mut u = init_universe_full();
    let s = default_sizes();
    let int = u.typ[BasicKind::Int as usize];
    let slice = new_slice(&mut u.type_arena, int);
    let ptr = new_pointer(&mut u.type_arena, int);

    // slice = 3 * word, pointer = word (catch-all).
    assert_eq!(
        s.sizeof(&u.type_arena, &u.object_arena, &u.package_arena, slice),
        24
    );
    assert_eq!(
        s.alignof(&u.type_arena, &u.object_arena, &u.package_arena, slice),
        8
    );
    assert_eq!(
        s.sizeof(&u.type_arena, &u.object_arena, &u.package_arena, ptr),
        8
    );
    assert_eq!(
        s.alignof(&u.type_arena, &u.object_arena, &u.package_arena, ptr),
        8
    );
}

#[test]
fn array_size_gc() {
    let mut u = init_universe_full();
    let s = default_sizes();
    let int32 = u.typ[BasicKind::Int32 as usize];
    let arr = new_array(&mut u.type_arena, int32, 4);
    // gc: esize * n = 4 * 4 = 16; align = align of elem = 4.
    assert_eq!(
        s.sizeof(&u.type_arena, &u.object_arena, &u.package_arena, arr),
        16
    );
    assert_eq!(
        s.alignof(&u.type_arena, &u.object_arena, &u.package_arena, arr),
        4
    );
}

#[test]
fn empty_array_is_zero() {
    let mut u = init_universe_full();
    let s = default_sizes();
    let int = u.typ[BasicKind::Int as usize];
    let arr = new_array(&mut u.type_arena, int, 0);
    assert_eq!(
        s.sizeof(&u.type_arena, &u.object_arena, &u.package_arena, arr),
        0
    );
    // Alignof of an array is the alignment of its element, even when empty.
    assert_eq!(
        s.alignof(&u.type_arena, &u.object_arena, &u.package_arena, arr),
        8
    );
}

#[test]
fn struct_offsets_and_size() {
    let mut u = init_universe_full();
    let s = default_sizes();
    let int8 = u.typ[BasicKind::Int8 as usize];
    let int64 = u.typ[BasicKind::Int64 as usize];
    let a = new_field(&mut u.object_arena, "a", int8, false);
    let b = new_field(&mut u.object_arena, "b", int64, false);
    let st = new_struct(&mut u.type_arena, vec![a, b], vec![]);

    let offsets = s.offsetsof(&u.type_arena, &u.object_arena, &u.package_arena, &[a, b]);
    assert_eq!(offsets, vec![0, 8]); // int8 at 0, int64 aligned to 8

    // align = max(1, 8) = 8; gc size = align(offs(8)+size(8), 8) = 16.
    assert_eq!(
        s.alignof(&u.type_arena, &u.object_arena, &u.package_arena, st),
        8
    );
    assert_eq!(
        s.sizeof(&u.type_arena, &u.object_arena, &u.package_arena, st),
        16
    );
}

#[test]
fn empty_struct_is_zero() {
    let mut u = init_universe_full();
    let s = default_sizes();
    let st = new_struct(&mut u.type_arena, vec![], vec![]);
    assert_eq!(
        s.sizeof(&u.type_arena, &u.object_arena, &u.package_arena, st),
        0
    );
    // Empty struct alignment is at least 1.
    assert_eq!(
        s.alignof(&u.type_arena, &u.object_arena, &u.package_arena, st),
        1
    );
}

#[test]
fn std_vs_gc_trailing_padding() {
    // struct{ a int64; b int8 }: offsets [0, 8], last field size 1.
    // StdSizes: offs + size = 8 + 1 = 9 (no trailing padding).
    // gcSizes:  align(8 + 1, 8) = 16 (size includes alignment padding).
    let mut u = init_universe_full();
    let int8 = u.typ[BasicKind::Int8 as usize];
    let int64 = u.typ[BasicKind::Int64 as usize];
    let a = new_field(&mut u.object_arena, "a", int64, false);
    let b = new_field(&mut u.object_arena, "b", int8, false);
    let st = new_struct(&mut u.type_arena, vec![a, b], vec![]);

    let std = Sizes::std(8, 8);
    let gc = Sizes::gc(8, 8);
    assert_eq!(
        std.sizeof(&u.type_arena, &u.object_arena, &u.package_arena, st),
        9
    );
    assert_eq!(
        gc.sizeof(&u.type_arena, &u.object_arena, &u.package_arena, st),
        16
    );
}

#[test]
fn align_function() {
    assert_eq!(align(0, 8), 0);
    assert_eq!(align(1, 8), 8);
    assert_eq!(align(8, 8), 8);
    assert_eq!(align(9, 8), 16);
    assert_eq!(align(3, 4), 4);
    assert_eq!(align(5, 2), 6);
    assert_eq!(align(7, 1), 7);
}

#[test]
fn sizes_for_known_and_unknown() {
    // gc -> gcSizes
    let gc = sizes_for("gc", "amd64").unwrap();
    assert_eq!(gc.kind, SizesKind::Gc);
    assert_eq!((gc.word_size, gc.max_align), (8, 8));
    let gc386 = sizes_for("gc", "386").unwrap();
    assert_eq!((gc386.word_size, gc386.max_align), (4, 4));

    // gccgo -> StdSizes
    let gccgo = sizes_for("gccgo", "386").unwrap();
    assert_eq!(gccgo.kind, SizesKind::Std);
    assert_eq!((gccgo.word_size, gccgo.max_align), (4, 4));
    let m68k = sizes_for("gccgo", "m68k").unwrap();
    assert_eq!((m68k.word_size, m68k.max_align), (4, 2));

    // unknown compiler / arch
    assert!(sizes_for("llvm", "amd64").is_none());
    assert!(sizes_for("gc", "no-such-arch").is_none());
}
