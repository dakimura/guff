//! Chunk-15 tests: `conversions.rs` — `convertible_to` (structural core) +
//! the `is_uintptr` / `is_unsafe_pointer` / `is_pointer` / `is_bytes_or_runes`
//! shape predicates.
//!
//! `assignable_to` is injected; unless a test specifically exercises the
//! short-circuit, it passes the always-false stub (`NEVER`) so the structural
//! rules are what's under test.

use guff_types::{
    convertible_to, init_universe_full, is_bytes_or_runes, is_pointer, is_uintptr,
    is_unsafe_pointer, new_array, new_interface_type, new_pointer, new_slice, new_term,
    new_type_name, new_type_param, new_union, BasicKind, ObjectArena, Operand, PackageArena,
    TypeArena, TypeId, Universe,
};

/// `assignable_to` stub that always reports "not assignable", so the
/// structural conversion rules alone decide the outcome.
const NEVER: &dyn Fn(&mut TypeArena, &ObjectArena, &PackageArena, &Operand, TypeId) -> bool =
    &|_, _, _, _, _| false;

fn op(typ: TypeId) -> Operand {
    let mut x = Operand::invalid();
    x.typ = Some(typ);
    x
}

fn b(u: &Universe, k: BasicKind) -> TypeId {
    u.typ[k as usize]
}

/// Convenience wrapper threading the universe arenas into `convertible_to`.
fn conv(u: &mut Universe, from: TypeId, to: TypeId) -> bool {
    let x = op(from);
    convertible_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        &x,
        to,
        NEVER,
    )
}

// ---------------------------------------------------------------------------
// shape predicates

#[test]
fn is_uintptr_true_only_for_uintptr() {
    let u = init_universe_full();
    assert!(is_uintptr(&u.type_arena, b(&u, BasicKind::Uintptr)));
    assert!(!is_uintptr(&u.type_arena, b(&u, BasicKind::Int)));
    assert!(!is_uintptr(&u.type_arena, b(&u, BasicKind::UnsafePointer)));
}

#[test]
fn is_unsafe_pointer_true_only_for_unsafe_pointer() {
    let u = init_universe_full();
    assert!(is_unsafe_pointer(
        &u.type_arena,
        b(&u, BasicKind::UnsafePointer)
    ));
    assert!(!is_unsafe_pointer(&u.type_arena, b(&u, BasicKind::Uintptr)));
}

#[test]
fn is_pointer_true_for_pointer_types() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let ptr = new_pointer(&mut u.type_arena, int);
    assert!(is_pointer(&u.type_arena, ptr));
    assert!(!is_pointer(&u.type_arena, int));
    // unsafe.Pointer is a Basic, not a structural pointer.
    assert!(!is_pointer(&u.type_arena, b(&u, BasicKind::UnsafePointer)));
}

#[test]
fn is_bytes_or_runes_true_for_byte_and_rune_slices() {
    let mut u = init_universe_full();
    let byte = b(&u, BasicKind::Uint8); // byte == uint8
    let rune = b(&u, BasicKind::Int32); // rune == int32
    let int = b(&u, BasicKind::Int);
    let bytes = new_slice(&mut u.type_arena, byte);
    let runes = new_slice(&mut u.type_arena, rune);
    let ints = new_slice(&mut u.type_arena, int);
    assert!(is_bytes_or_runes(&u.type_arena, bytes));
    assert!(is_bytes_or_runes(&u.type_arena, runes));
    assert!(!is_bytes_or_runes(&u.type_arena, ints));
    assert!(!is_bytes_or_runes(&u.type_arena, byte));
}

// ---------------------------------------------------------------------------
// numeric / complex

#[test]
fn integer_to_float_and_back_is_convertible() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let f64_ = b(&u, BasicKind::Float64);
    assert!(conv(&mut u, int, f64_));
    assert!(conv(&mut u, f64_, int));
}

#[test]
fn complex_to_complex_is_convertible() {
    let mut u = init_universe_full();
    let c64 = b(&u, BasicKind::Complex64);
    let c128 = b(&u, BasicKind::Complex128);
    assert!(conv(&mut u, c64, c128));
    // but float to complex is not a (direct) conversion
    let f64_ = b(&u, BasicKind::Float64);
    assert!(!conv(&mut u, f64_, c128));
}

// ---------------------------------------------------------------------------
// string ⇄ integer / bytes / runes

#[test]
fn integer_to_string_is_convertible() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let string = b(&u, BasicKind::String);
    assert!(conv(&mut u, int, string));
    // string to a plain integer is NOT convertible
    assert!(!conv(&mut u, string, int));
}

#[test]
fn string_and_byte_or_rune_slices_interconvert() {
    let mut u = init_universe_full();
    let string = b(&u, BasicKind::String);
    let byte = b(&u, BasicKind::Uint8);
    let rune = b(&u, BasicKind::Int32);
    let bytes = new_slice(&mut u.type_arena, byte);
    let runes = new_slice(&mut u.type_arena, rune);

    assert!(conv(&mut u, string, bytes)); // string -> []byte
    assert!(conv(&mut u, string, runes)); // string -> []rune
    assert!(conv(&mut u, bytes, string)); // []byte -> string
    assert!(conv(&mut u, runes, string)); // []rune -> string
}

// ---------------------------------------------------------------------------
// identical underlying types

#[test]
fn identical_underlying_slices_are_convertible() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let s1 = new_slice(&mut u.type_arena, int);
    let s2 = new_slice(&mut u.type_arena, int);
    // Two anonymous []int have identical underlying types.
    assert!(conv(&mut u, s1, s2));
}

#[test]
fn unrelated_composites_are_not_convertible() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let string = b(&u, BasicKind::String);
    let si = new_slice(&mut u.type_arena, int);
    // int -> []int is not a conversion
    assert!(!conv(&mut u, int, si));
    // []int -> string is not a conversion ([]byte/[]rune only)
    assert!(!conv(&mut u, si, string));
}

// ---------------------------------------------------------------------------
// pointer rules

#[test]
fn unnamed_pointers_with_identical_base_are_convertible() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let p1 = new_pointer(&mut u.type_arena, int);
    let p2 = new_pointer(&mut u.type_arena, int);
    assert!(conv(&mut u, p1, p2));
}

#[test]
fn pointers_with_distinct_base_are_not_convertible() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let string = b(&u, BasicKind::String);
    let pi = new_pointer(&mut u.type_arena, int);
    let ps = new_pointer(&mut u.type_arena, string);
    assert!(!conv(&mut u, pi, ps));
}

// ---------------------------------------------------------------------------
// unsafe.Pointer rules

#[test]
fn unsafe_pointer_interconverts_with_pointers_and_uintptr() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let unsafe_ptr = b(&u, BasicKind::UnsafePointer);
    let uintptr = b(&u, BasicKind::Uintptr);
    let pi = new_pointer(&mut u.type_arena, int);

    assert!(conv(&mut u, pi, unsafe_ptr)); // *int -> unsafe.Pointer
    assert!(conv(&mut u, uintptr, unsafe_ptr)); // uintptr -> unsafe.Pointer
    assert!(conv(&mut u, unsafe_ptr, pi)); // unsafe.Pointer -> *int
    assert!(conv(&mut u, unsafe_ptr, uintptr)); // unsafe.Pointer -> uintptr
}

// ---------------------------------------------------------------------------
// slice -> array / pointer-to-array (Go 1.20 / 1.17; check==nil ⇒ allowed)

#[test]
fn slice_to_array_and_array_pointer_is_convertible() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let si = new_slice(&mut u.type_arena, int);
    let arr = new_array(&mut u.type_arena, int, 3);
    let parr = new_pointer(&mut u.type_arena, arr);

    assert!(conv(&mut u, si, arr)); // []int -> [3]int
    assert!(conv(&mut u, si, parr)); // []int -> *[3]int

    // element-type mismatch is not convertible
    let string = b(&u, BasicKind::String);
    let arr_s = new_array(&mut u.type_arena, string, 3);
    assert!(!conv(&mut u, si, arr_s));
}

// ---------------------------------------------------------------------------
// assignable_to short-circuit

#[test]
fn assignable_short_circuits_to_convertible() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let string = b(&u, BasicKind::String);
    // string -> int is structurally NOT convertible...
    assert!(!conv(&mut u, string, int));
    // ...but if the injected assignable_to says yes, convertible_to says yes.
    let always: &dyn Fn(&mut TypeArena, &ObjectArena, &PackageArena, &Operand, TypeId) -> bool =
        &|_, _, _, _, _| true;
    let x = op(string);
    assert!(convertible_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        &x,
        int,
        always,
    ));
}

// ---------------------------------------------------------------------------
// generic (type-parameter) cases

#[test]
fn convert_to_type_param_requires_all_terms_convertible() {
    // T's constraint is `interface { int | float64 }`. A value of type int
    // is convertible to T because int is convertible to each specific term.
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let f64_ = b(&u, BasicKind::Float64);

    let t_int = new_term(false, int);
    let t_f64 = new_term(false, f64_);
    let union = new_union(&mut u.type_arena, vec![t_int, t_f64]);
    let iface = new_interface_type(&mut u.type_arena, vec![], vec![union]);
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, Some(iface));

    // int -> T : int convertible to both int and float64 ⇒ true
    assert!(conv(&mut u, int, tp));

    // string -> T : string is not convertible to int/float64 ⇒ false
    let string = b(&u, BasicKind::String);
    assert!(!conv(&mut u, string, tp));
}

#[test]
fn convert_from_type_param_with_no_terms_is_not_convertible() {
    // T is `any` — no specific type terms ⇒ a value of type T cannot be
    // converted to a concrete type via the structural rules.
    let mut u = init_universe_full();
    let empty_iface = new_interface_type(&mut u.type_arena, vec![], vec![]);
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, Some(empty_iface));

    let int = b(&u, BasicKind::Int);
    assert!(!conv(&mut u, tp, int));
}
