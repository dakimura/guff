//! Chunk-3 tests: TypeName / TypeParam / Alias / Named, including cyclic
//! construction (the whole point of the arena design).

use guff_types::{
    add_method, alias_rhs, bind_tparams, init_universe, named_method, named_num_methods, named_obj,
    named_underlying, new_alias, new_func, new_named, new_pointer, new_signature_type, new_slice,
    new_struct, new_type_name, new_type_param, new_var, set_constraint, set_underlying,
    type_param_constraint, type_param_index, type_param_obj, unalias, BasicKind, ObjectArena,
    ObjectData, TypeKind,
};

#[test]
fn type_name_two_phase_construction() {
    let (mut t_arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let mut o_arena = ObjectArena::new();
    let tn = new_type_name(&mut o_arena, "MyInt", None);
    assert_eq!(tn.name(&o_arena), "MyInt");
    assert_eq!(tn.typ(&o_arena), None);

    // Build a Named bound to this TypeName — Named's constructor back-fills
    // TypeName.typ automatically.
    let named = new_named(&mut t_arena, &mut o_arena, tn, Some(int), vec![]);
    assert_eq!(tn.typ(&o_arena), Some(named));
}

#[test]
fn typeparam_construction_and_binding() {
    let (mut t_arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let mut o_arena = ObjectArena::new();
    let tn_p = new_type_name(&mut o_arena, "P", None);
    let tp = new_type_param(&mut t_arena, tn_p, None);
    assert_eq!(tp.kind(&t_arena), TypeKind::TypeParam);
    assert_eq!(type_param_obj(&t_arena, tp), tn_p);
    assert_eq!(type_param_index(&t_arena, tp), -1);
    assert_eq!(type_param_constraint(&t_arena, tp), None);

    // SetConstraint mutates the bound.
    set_constraint(&mut t_arena, tp, int);
    assert_eq!(type_param_constraint(&t_arena, tp), Some(int));

    // bind_tparams sets the index.
    let list = bind_tparams(&mut t_arena, vec![tp]).expect("non-empty");
    assert_eq!(list.len(), 1);
    assert_eq!(list.at(0), tp);
    assert_eq!(type_param_index(&t_arena, tp), 0);
}

#[test]
#[should_panic(expected = "bound more than once")]
fn typeparam_double_bind_panics() {
    let mut t_arena = init_universe().0;
    let mut o_arena = ObjectArena::new();
    let tn = new_type_name(&mut o_arena, "P", None);
    let tp = new_type_param(&mut t_arena, tn, None);
    bind_tparams(&mut t_arena, vec![tp]).unwrap();
    // Second bind panics.
    bind_tparams(&mut t_arena, vec![tp]);
}

#[test]
fn alias_chain_unalias_and_underlying() {
    let (mut t_arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let mut o_arena = ObjectArena::new();
    // type A = int
    let tn_a = new_type_name(&mut o_arena, "A", None);
    let alias_a = new_alias(&mut t_arena, &mut o_arena, tn_a, Some(int));
    assert_eq!(alias_a.kind(&t_arena), TypeKind::Alias);
    assert_eq!(alias_rhs(&t_arena, alias_a), Some(int));

    // type B = A — chain through to int
    let tn_b = new_type_name(&mut o_arena, "B", None);
    let alias_b = new_alias(&mut t_arena, &mut o_arena, tn_b, Some(alias_a));
    assert_eq!(unalias(&mut t_arena, alias_b), int);

    // underlying() also resolves the chain.
    assert_eq!(alias_b.underlying(&t_arena), int);
    assert_eq!(alias_a.underlying(&t_arena), int);

    // unalias on a non-alias is identity.
    assert_eq!(unalias(&mut t_arena, int), int);
}

#[test]
fn named_two_phase_construction_with_cycle() {
    // `type T struct { next *T }` — the classic recursive type.
    let (mut t_arena, _) = init_universe();
    let mut o_arena = ObjectArena::new();

    // Phase 1: allocate TypeName and incomplete Named so we have a TypeId
    // for `T` before we build the struct that references it.
    let tn = new_type_name(&mut o_arena, "T", None);
    let named_t = new_named(&mut t_arena, &mut o_arena, tn, None, vec![]);
    assert_eq!(named_t.kind(&t_arena), TypeKind::Named);
    assert_eq!(named_underlying(&t_arena, named_t), None);
    // Incomplete Named's underlying() is self (chunk-3 semantics).
    assert_eq!(named_t.underlying(&t_arena), named_t);

    // Phase 2: build *T, the struct field, the struct, and patch in.
    let ptr_to_t = new_pointer(&mut t_arena, named_t);
    let field_next = new_var(&mut o_arena, "next", ptr_to_t);
    let s = new_struct(&mut t_arena, vec![field_next], vec![]);

    set_underlying(&mut t_arena, named_t, s);
    assert_eq!(named_underlying(&t_arena, named_t), Some(s));
    assert_eq!(named_t.underlying(&t_arena), s);
}

#[test]
fn named_add_method_dedupes_by_name() {
    let (mut t_arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let mut o_arena = ObjectArena::new();
    let tn = new_type_name(&mut o_arena, "T", None);
    let named = new_named(&mut t_arena, &mut o_arena, tn, Some(int), vec![]);

    // Build two methods named "Foo" (with trivial signatures).
    let sig = new_signature_type(&mut t_arena, None, &[], &[], None, None, false);
    let m1 = new_func(&mut o_arena, "Foo", Some(sig));
    let m2 = new_func(&mut o_arena, "Foo", Some(sig));
    let m3 = new_func(&mut o_arena, "Bar", Some(sig));

    assert!(add_method(&mut t_arena, &o_arena, named, m1));
    // Same name → rejected.
    assert!(!add_method(&mut t_arena, &o_arena, named, m2));
    // Different name → added.
    assert!(add_method(&mut t_arena, &o_arena, named, m3));

    assert_eq!(named_num_methods(&t_arena, named), 2);
    assert_eq!(named_method(&t_arena, named, 0), m1);
    assert_eq!(named_method(&t_arena, named, 1), m3);
}

#[test]
fn named_obj_back_reference_round_trips() {
    let (mut t_arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let mut o_arena = ObjectArena::new();
    let tn = new_type_name(&mut o_arena, "Counter", None);
    let n = new_named(&mut t_arena, &mut o_arena, tn, Some(int), vec![]);

    // Named → TypeName → Named (the cycle goes the other direction here:
    // tn.typ points back to n).
    assert_eq!(named_obj(&t_arena, n), tn);
    assert_eq!(tn.typ(&o_arena), Some(n));
    match o_arena.get(tn) {
        ObjectData::TypeName(t) => assert_eq!(t.name(), "Counter"),
        _ => panic!("expected TypeName"),
    }
}

#[test]
#[should_panic(expected = "underlying type must not be *Named")]
fn named_underlying_cannot_be_named() {
    let (mut t_arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let mut o_arena = ObjectArena::new();
    let tn1 = new_type_name(&mut o_arena, "A", None);
    let n1 = new_named(&mut t_arena, &mut o_arena, tn1, Some(int), vec![]);

    let tn2 = new_type_name(&mut o_arena, "B", None);
    // n1 is itself a Named → must panic.
    new_named(&mut t_arena, &mut o_arena, tn2, Some(n1), vec![]);
}

#[test]
fn underlying_dispatches_through_named_to_alias_to_basic() {
    // `type A = int; type B A` — B is a Named whose underlying is int
    // (Aliases are not Named, so this is allowed: `type B A` where A is
    // an alias for int gives Named B with underlying int).
    let (mut t_arena, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let mut o_arena = ObjectArena::new();
    let tn_a = new_type_name(&mut o_arena, "A", None);
    let alias_a = new_alias(&mut t_arena, &mut o_arena, tn_a, Some(int));

    // Resolve A to int *before* using it as B's underlying (since Named
    // underlyings can't be Named, but Alias→int is fine; in fact, after
    // unalias we have int, which is what we'd pass to new_named).
    let resolved = unalias(&mut t_arena, alias_a);
    assert_eq!(resolved, int);

    let tn_b = new_type_name(&mut o_arena, "B", None);
    let named_b = new_named(&mut t_arena, &mut o_arena, tn_b, Some(resolved), vec![]);
    assert_eq!(named_b.underlying(&t_arena), int);
}

#[test]
fn slice_of_typeparam_is_valid() {
    // Sanity: a type that *contains* a TypeParam (like `[]P`) constructs
    // and accesses normally — TypeParam acts as a regular TypeId.
    let mut t_arena = init_universe().0;
    let mut o_arena = ObjectArena::new();
    let tn_p = new_type_name(&mut o_arena, "P", None);
    let p = new_type_param(&mut t_arena, tn_p, None);
    let slice_of_p = new_slice(&mut t_arena, p);
    assert_eq!(slice_of_p.kind(&t_arena), TypeKind::Slice);
    assert_eq!(guff_types::slice_elem(&t_arena, slice_of_p), p);
}
