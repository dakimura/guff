//! Type-canonicalization tests (Milestone E, chunk E08).
//!
//! Exercises `Canonizer`: two structurally-identical but distinct-`TypeId` types
//! map to one representative, a top-level alias is stripped, and type-arg lists
//! that are pairwise identical share a `CanonListId` while different lists do not.

use guff_ssa::canon::Canonizer;
use guff_types::{
    basic::{init_universe, BasicKind},
    new_alias, new_field,
    object::type_name::new_type_name,
    r#struct::new_struct,
    slice::new_slice,
    ObjectArena, PackageArena,
};

/// Two independently-built but identical `struct{ x int }` types canonicalize to
/// the same representative; a `string` does not.
#[test]
fn test_canonical_type_dedup() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let string_ty = table[BasicKind::String as usize];

    let mut objs = ObjectArena::new();
    let parena = PackageArena::new();

    // Two distinct struct TypeIds with identical shape (fresh field objects).
    let f1 = new_field(&mut objs, "x", int_ty, false);
    let s1 = new_struct(&mut arena, vec![f1], vec![String::new()]);
    let f2 = new_field(&mut objs, "x", int_ty, false);
    let s2 = new_struct(&mut arena, vec![f2], vec![String::new()]);
    assert_ne!(s1, s2, "the two structs are distinct TypeIds");

    let mut canon = Canonizer::default();
    let r1 = canon.canonical_type(&mut arena, &objs, &parena, s1);
    let r2 = canon.canonical_type(&mut arena, &objs, &parena, s2);
    assert_eq!(r1, r2, "identical structs share a canonical representative");
    assert_eq!(r1, s1, "the first seen becomes the representative");

    // An unrelated type gets its own representative.
    let rs = canon.canonical_type(&mut arena, &objs, &parena, string_ty);
    assert_ne!(rs, r1);
}

/// The top-level alias is removed: `type A = int` canonicalizes to `int`.
#[test]
fn test_canonical_type_unaliases() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];

    let mut objs = ObjectArena::new();
    let parena = PackageArena::new();

    let a_obj = new_type_name(&mut objs, "A", None);
    let a_alias = new_alias(&mut arena, &mut objs, a_obj, Some(int_ty));

    let mut canon = Canonizer::default();
    let ri = canon.canonical_type(&mut arena, &objs, &parena, int_ty);
    let ra = canon.canonical_type(&mut arena, &objs, &parena, a_alias);
    assert_eq!(ra, ri, "a top-level alias canonicalizes to its aliasee");
}

/// Type-arg lists share an id iff pairwise identical; the empty list has none.
#[test]
fn test_canonical_list() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let string_ty = table[BasicKind::String as usize];

    let objs = ObjectArena::new();
    let parena = PackageArena::new();
    let mut canon = Canonizer::default();

    // Distinct-but-identical []int elements → same list id.
    let a = new_slice(&mut arena, int_ty);
    let b = new_slice(&mut arena, int_ty);
    let l1 = canon
        .canonical_list(&mut arena, &objs, &parena, &[a])
        .unwrap();
    let l2 = canon
        .canonical_list(&mut arena, &objs, &parena, &[b])
        .unwrap();
    assert_eq!(l1, l2, "identical single-element lists share an id");

    // A different element → different id.
    let l3 = canon
        .canonical_list(&mut arena, &objs, &parena, &[string_ty])
        .unwrap();
    assert_ne!(l1, l3);

    // Multi-element ordering matters.
    let l_is = canon
        .canonical_list(&mut arena, &objs, &parena, &[int_ty, string_ty])
        .unwrap();
    let l_si = canon
        .canonical_list(&mut arena, &objs, &parena, &[string_ty, int_ty])
        .unwrap();
    assert_ne!(l_is, l_si);

    // The empty list has no representative.
    assert!(canon
        .canonical_list(&mut arena, &objs, &parena, &[])
        .is_none());
}
