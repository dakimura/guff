//! Type-set utility tests (Milestone E, chunk E09).
//!
//! Exercises the go/ssa `typeset.go` port: `typeset`/`typeset_pairs`,
//! `typeset_is_empty`, `under_is`, `is_bytestring`, and `index_type` +
//! `IndexMode::meet`.

use guff_ssa::typeset::{
    index_type, is_bytestring, typeset_is_empty, typeset_pairs, under_is, IndexMode,
};
use guff_types::{
    array::new_array,
    basic::{init_universe, BasicKind},
    interface::new_interface_type,
    map::new_map,
    object::type_name::new_type_name,
    pointer::new_pointer,
    slice::new_slice,
    typeparam::new_type_param,
    union::{new_term, new_union},
    ObjectArena,
};

/// A non-type-parameter, non-interface type has a singleton type set:
/// `(type, underlying)`.
#[test]
fn test_typeset_concrete() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let objs = ObjectArena::new();

    // int → single pair (int, int).
    let pairs = typeset_pairs(&mut arena, &objs, &Default::default(), int_ty);
    assert_eq!(pairs, vec![(Some(int_ty), Some(int_ty))]);

    // []int → single pair (slice, slice) — an unnamed composite is its own
    // underlying.
    let slice_int = new_slice(&mut arena, int_ty);
    let pairs = typeset_pairs(&mut arena, &objs, &Default::default(), slice_int);
    assert_eq!(pairs, vec![(Some(slice_int), Some(slice_int))]);

    // Not empty.
    assert!(!typeset_is_empty(&mut arena, &objs, &Default::default(), int_ty));
}

/// `interface{ ~int | string }` expands into its term list; a type parameter
/// so constrained expands the same way.
#[test]
fn test_typeset_interface_and_typeparam() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let string_ty = table[BasicKind::String as usize];
    let mut objs = ObjectArena::new();

    // interface{ ~int | string } — the union is the sole embedded element.
    let union_ty = new_union(
        &mut arena,
        vec![new_term(true, int_ty), new_term(false, string_ty)],
    );
    let iface = new_interface_type(&mut arena, vec![], vec![union_ty]);

    let ts: Vec<Option<guff_types::TypeId>> =
        typeset_pairs(&mut arena, &objs, &Default::default(), iface)
            .into_iter()
            .map(|(t, _)| t)
            .collect();
    assert_eq!(ts.len(), 2, "two terms");
    assert!(ts.contains(&Some(int_ty)) && ts.contains(&Some(string_ty)));
    assert!(!typeset_is_empty(&mut arena, &objs, &Default::default(), iface));

    // A type parameter constrained by that interface expands identically.
    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, Some(iface));
    let ts2: Vec<Option<guff_types::TypeId>> =
        typeset_pairs(&mut arena, &objs, &Default::default(), tparam)
            .into_iter()
            .map(|(t, _)| t)
            .collect();
    assert_eq!(ts2.len(), 2, "type-parameter constraint expands to the same terms");
    assert!(ts2.contains(&Some(int_ty)) && ts2.contains(&Some(string_ty)));

    // The `~int` term yields underlying int (tilde: unalias only); the `string`
    // term yields underlying string.
    for (t, u) in typeset_pairs(&mut arena, &objs, &Default::default(), iface) {
        assert_eq!(t, u, "int/string are their own underlyings");
    }
}

/// The empty interface (`any`) has the *all* type set, which — matching go/ssa
/// `termListOf` returning an empty slice for it — yields a single `(None,
/// None)` pair, and an interface with disjoint terms has a genuinely empty
/// type set. Both report `typeset_is_empty == true`.
#[test]
fn test_typeset_empty_and_all() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let bool_ty = table[BasicKind::Bool as usize];
    let objs = ObjectArena::new();

    // any — all types, no specific terms.
    let any_ty = new_interface_type(&mut arena, vec![], vec![]);
    assert_eq!(
        typeset_pairs(&mut arena, &objs, &Default::default(), any_ty),
        vec![(None, None)]
    );
    assert!(typeset_is_empty(&mut arena, &objs, &Default::default(), any_ty));

    // interface{ int; bool } — intersection of {int} and {bool} is empty.
    let empty_iface = new_interface_type(&mut arena, vec![], vec![int_ty, bool_ty]);
    assert_eq!(
        typeset_pairs(&mut arena, &objs, &Default::default(), empty_iface),
        vec![(None, None)]
    );
    assert!(typeset_is_empty(&mut arena, &objs, &Default::default(), empty_iface));
}

/// `under_is` reports whether all underlyings satisfy the predicate, and calls
/// it once with `None` for a set with no specific terms.
#[test]
fn test_under_is() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let string_ty = table[BasicKind::String as usize];
    let objs = ObjectArena::new();

    // interface{ int | string } — all terms are basic.
    let union_ty = new_union(
        &mut arena,
        vec![new_term(false, int_ty), new_term(false, string_ty)],
    );
    let iface = new_interface_type(&mut arena, vec![], vec![union_ty]);
    let all_basic = under_is(&mut arena, &objs, &Default::default(), iface, |arena, u| {
        matches!(u, Some(x) if matches!(arena.get(x), guff_types::TypeData::Basic(_)))
    });
    assert!(all_basic);

    // Not all terms are strings.
    let all_string = under_is(&mut arena, &objs, &Default::default(), iface, |arena, u| {
        u.is_some_and(|x| guff_types::is_string(arena, x))
    });
    assert!(!all_string);

    // Empty term set → f(None) governs the result.
    let any_ty = new_interface_type(&mut arena, vec![], vec![]);
    let saw_none = under_is(&mut arena, &objs, &Default::default(), any_ty, |_, u| u.is_none());
    assert!(saw_none);
}

/// `is_bytestring` matches exactly `interface{ []byte | string }`.
#[test]
fn test_is_bytestring() {
    let (mut arena, table) = init_universe();
    let byte_ty = table[BasicKind::Uint8 as usize];
    let string_ty = table[BasicKind::String as usize];
    let int_ty = table[BasicKind::Int as usize];
    let objs = ObjectArena::new();

    // interface{ []byte | string } → true.
    let byte_slice = new_slice(&mut arena, byte_ty);
    let union_ty = new_union(
        &mut arena,
        vec![new_term(false, byte_slice), new_term(false, string_ty)],
    );
    let bytestring = new_interface_type(&mut arena, vec![], vec![union_ty]);
    assert!(is_bytestring(&mut arena, &objs, &Default::default(), bytestring));

    // interface{ ~int | string } → false (has int, not []byte).
    let union2 = new_union(
        &mut arena,
        vec![new_term(true, int_ty), new_term(false, string_ty)],
    );
    let not_bs = new_interface_type(&mut arena, vec![], vec![union2]);
    assert!(!is_bytestring(&mut arena, &objs, &Default::default(), not_bs));

    // A plain (non-interface) type → false.
    assert!(!is_bytestring(&mut arena, &objs, &Default::default(), string_ty));
}

/// `index_type` returns element type and addressing mode per indexable kind.
#[test]
fn test_index_type() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let string_ty = table[BasicKind::String as usize];
    let byte_ty = table[BasicKind::Uint8 as usize];
    let objs = ObjectArena::new();

    // [3]int → (int, ArrVar).
    let arr = new_array(&mut arena, int_ty, 3);
    assert_eq!(
        index_type(&mut arena, &objs, &Default::default(), arr),
        (Some(int_ty), IndexMode::ArrVar)
    );

    // []int → (int, Var).
    let slc = new_slice(&mut arena, int_ty);
    assert_eq!(
        index_type(&mut arena, &objs, &Default::default(), slc),
        (Some(int_ty), IndexMode::Var)
    );

    // map[string]int → (int, Map).
    let mp = new_map(&mut arena, string_ty, int_ty);
    assert_eq!(
        index_type(&mut arena, &objs, &Default::default(), mp),
        (Some(int_ty), IndexMode::Map)
    );

    // string → (byte, Value).
    assert_eq!(
        index_type(&mut arena, &objs, &Default::default(), string_ty),
        (Some(byte_ty), IndexMode::Value)
    );

    // *[3]int → (int, Var).
    let ptr_arr = new_pointer(&mut arena, arr);
    assert_eq!(
        index_type(&mut arena, &objs, &Default::default(), ptr_arr),
        (Some(int_ty), IndexMode::Var)
    );

    // *int (pointer to non-array) → not indexable.
    let ptr_int = new_pointer(&mut arena, int_ty);
    assert_eq!(
        index_type(&mut arena, &objs, &Default::default(), ptr_int),
        (None, IndexMode::Invalid)
    );
}

/// The `IndexMode` meet semi-lattice.
#[test]
fn test_index_mode_meet() {
    use IndexMode::*;
    // Map is incompatible with anything but itself.
    assert_eq!(Var.meet(Map), Invalid);
    assert_eq!(Map.meet(Var), Invalid);
    assert_eq!(Map.meet(Map), Map);
    // Otherwise, the more-constrained (larger) mode wins.
    assert_eq!(Value.meet(ArrVar), ArrVar);
    assert_eq!(ArrVar.meet(Var), Var);
    assert_eq!(Invalid.meet(Value), Value);
    assert_eq!(Var.meet(Var), Var);
}
