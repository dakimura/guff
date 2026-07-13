//! Chunk-9 tests: Context dedup, substitution, Instantiate, instantiated Named identity.

use guff_types::{
    add_method, alias_origin, alias_set_type_params, bind_tparams, identical, init_universe,
    instantiate, make_subst_map, named_origin, named_set_type_params, named_type_args, new_alias,
    new_field, new_func, new_named, new_signature_type, new_slice, new_struct, new_type_name,
    new_type_param, new_var, set_underlying, signature_params, signature_results,
    signature_set_type_params, BasicKind, Context, ObjectArena, ObjectData, PackageArena, TypeData,
    TypeKind,
};

// ----------------------------------------------------------------------------
// Context dedup

#[test]
fn context_dedups_same_origin_and_targs() {
    let (mut t, table) = init_universe();
    let mut o = ObjectArena::new();
    let mut ctxt = Context::new();
    let int = table[BasicKind::Int as usize];

    // Build a generic Named: type Vec[T any] []T
    let tn_t = new_type_name(&mut o, "T", None);
    let tp = new_type_param(&mut t, tn_t, None);
    let tlist = bind_tparams(&mut t, vec![tp]).unwrap();

    let tn_vec = new_type_name(&mut o, "Vec", None);
    let slice_of_t = new_slice(&mut t, tp);
    let vec_named = new_named(&mut t, &mut o, tn_vec, Some(slice_of_t), vec![]);
    named_set_type_params(&mut t, vec_named, tlist);

    let v1 = instantiate(&mut t, &mut o, &mut ctxt, vec_named, vec![int]);
    let v2 = instantiate(&mut t, &mut o, &mut ctxt, vec_named, vec![int]);
    assert_eq!(v1, v2, "second instantiation should dedup");

    let str_typ = table[BasicKind::String as usize];
    let v3 = instantiate(&mut t, &mut o, &mut ctxt, vec_named, vec![str_typ]);
    assert_ne!(v1, v3, "different targs → different instance");
}

// ----------------------------------------------------------------------------
// Substitution (direct)

#[test]
fn subst_replaces_typeparam_with_concrete_in_slice() {
    let (mut t, table) = init_universe();
    let mut o = ObjectArena::new();
    let mut ctxt = Context::new();
    let int = table[BasicKind::Int as usize];

    let tn = new_type_name(&mut o, "T", None);
    let tp = new_type_param(&mut t, tn, None);
    let slice_of_t = new_slice(&mut t, tp);

    // subst([]T, T=int) → []int (a freshly-allocated Slice).
    let smap = make_subst_map(&[tp], &[int]);
    let result = guff_types::subst(&mut t, &mut o, &smap, None, &mut ctxt, slice_of_t);
    assert_ne!(result, slice_of_t);
    assert_eq!(result.kind(&t), TypeKind::Slice);
    assert_eq!(guff_types::slice_elem(&t, result), int);
}

#[test]
fn subst_returns_unchanged_for_unrelated_types() {
    let (mut t, table) = init_universe();
    let mut o = ObjectArena::new();
    let mut ctxt = Context::new();
    let int = table[BasicKind::Int as usize];
    let str_typ = table[BasicKind::String as usize];

    let tn = new_type_name(&mut o, "T", None);
    let tp = new_type_param(&mut t, tn, None);
    let slice_of_str = new_slice(&mut t, str_typ);

    // smap maps T → int, but our slice is []string (no T inside) — no
    // change, same TypeId.
    let smap = make_subst_map(&[tp], &[int]);
    let result = guff_types::subst(&mut t, &mut o, &smap, None, &mut ctxt, slice_of_str);
    assert_eq!(result, slice_of_str);
}

// ----------------------------------------------------------------------------
// Instantiate Named

#[test]
fn instantiate_named_with_struct_underlying() {
    // type Box[T any] struct { v T }
    let (mut t, table) = init_universe();
    let mut o = ObjectArena::new();
    let mut ctxt = Context::new();
    let int = table[BasicKind::Int as usize];

    let tn_t = new_type_name(&mut o, "T", None);
    let tp = new_type_param(&mut t, tn_t, None);
    let tlist = bind_tparams(&mut t, vec![tp]).unwrap();

    let v_field = new_field(&mut o, "v", tp, false);
    let struct_with_t = new_struct(&mut t, vec![v_field], vec![]);

    let tn_box = new_type_name(&mut o, "Box", None);
    let box_named = new_named(&mut t, &mut o, tn_box, Some(struct_with_t), vec![]);
    named_set_type_params(&mut t, box_named, tlist);

    // Instantiate Box[int]
    let box_int = instantiate(&mut t, &mut o, &mut ctxt, box_named, vec![int]);
    assert_eq!(box_int.kind(&t), TypeKind::Named);
    assert_ne!(box_int, box_named, "instance is a distinct TypeId");

    // The instance's TypeArgs should be [int].
    let targs = named_type_args(&t, box_int).expect("instance has targs");
    assert_eq!(targs.list(), &[int]);
    // Origin walks back to box_named.
    assert_eq!(named_origin(&t, box_int), box_named);
    // The origin's TypeArgs are None (it's not an instance).
    assert!(named_type_args(&t, box_named).is_none());

    // Underlying of the instance: struct { v int } — a freshly-built Struct.
    let underlying = box_int.underlying(&t);
    assert_eq!(underlying.kind(&t), TypeKind::Struct);
    let n_fields = guff_types::struct_num_fields(&t, underlying);
    assert_eq!(n_fields, 1);
    let f0 = guff_types::struct_field(&t, underlying, 0);
    assert_eq!(f0.typ(&o), Some(int));
}

#[test]
fn instantiate_named_preserves_recursive_pointer_via_context() {
    // type T[P any] struct { next *T[P] } — self-referential through
    // T[P]. Instantiating T[int] should NOT loop: the Context catches
    // the recursive lookup of T[int].
    //
    // Setup: build T with a placeholder underlying that includes
    // *T[P] (where T[P] is the explicit self-instance).
    let mut t = init_universe().0;
    let mut o = ObjectArena::new();
    let mut ctxt = Context::new();

    // 1. Allocate TypeName + Named placeholder.
    let tn_t = new_type_name(&mut o, "T", None);
    let tn_p = new_type_name(&mut o, "P", None);
    let tp = new_type_param(&mut t, tn_p, None);
    let tlist = bind_tparams(&mut t, vec![tp]).unwrap();

    let named_t = new_named(&mut t, &mut o, tn_t, None, vec![]);
    named_set_type_params(&mut t, named_t, tlist);

    // 2. Build *T[P] (referencing the generic origin).
    let ptr_to_t = guff_types::new_pointer(&mut t, named_t);
    let next_field = new_field(&mut o, "next", ptr_to_t, false);
    let inner_struct = new_struct(&mut t, vec![next_field], vec![]);
    set_underlying(&mut t, named_t, inner_struct);

    // 3. Instantiate T[int].
    let (_, table) = init_universe();
    let int = table[BasicKind::Int as usize];
    let t_int = instantiate(&mut t, &mut o, &mut ctxt, named_t, vec![int]);
    assert_eq!(t_int.kind(&t), TypeKind::Named);
    // The chunk-9 substituter doesn't change a `*T_origin` reference
    // unless T_origin itself is also instantiated (it's a different
    // TypeId from T[P]). For now, we just assert that instantiation
    // completes without looping and produces a valid instance.
    assert_eq!(named_origin(&t, t_int), named_t);
}

// ----------------------------------------------------------------------------
// Instantiate Alias

#[test]
fn instantiate_alias_substitutes_rhs() {
    // type Pair[T any] = struct { a T; b T }
    let mut t = init_universe().0;
    let mut o = ObjectArena::new();
    let mut ctxt = Context::new();

    let tn_t = new_type_name(&mut o, "T", None);
    let tp = new_type_param(&mut t, tn_t, None);
    let tlist = bind_tparams(&mut t, vec![tp]).unwrap();

    let a_field = new_field(&mut o, "a", tp, false);
    let b_field = new_field(&mut o, "b", tp, false);
    let pair_struct = new_struct(&mut t, vec![a_field, b_field], vec![]);

    let tn_pair = new_type_name(&mut o, "Pair", None);
    let pair_alias = new_alias(&mut t, &mut o, tn_pair, Some(pair_struct));
    alias_set_type_params(&mut t, pair_alias, tlist);

    let (_, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let pair_int = instantiate(&mut t, &mut o, &mut ctxt, pair_alias, vec![int]);
    assert_eq!(pair_int.kind(&t), TypeKind::Alias);
    assert_eq!(alias_origin(&t, pair_int), pair_alias);

    // Alias underlying should resolve to a struct { a int; b int }.
    let underlying = pair_int.underlying(&t);
    assert_eq!(underlying.kind(&t), TypeKind::Struct);
}

// ----------------------------------------------------------------------------
// Instantiate Signature

#[test]
fn instantiate_signature_substitutes_params_and_results() {
    // func F[T any](x T) T
    let mut t = init_universe().0;
    let mut o = ObjectArena::new();
    let mut ctxt = Context::new();

    let tn_t = new_type_name(&mut o, "T", None);
    let tp = new_type_param(&mut t, tn_t, None);
    let tlist = bind_tparams(&mut t, vec![tp]).unwrap();

    let param_x = new_var(&mut o, "x", tp);
    let params = guff_types::new_tuple(&mut t, &[param_x]);
    let result = new_var(&mut o, "", tp);
    let results = guff_types::new_tuple(&mut t, &[result]);
    let sig = new_signature_type(&mut t, None, &[], &[], params, results, false);
    signature_set_type_params(&mut t, sig, tlist);

    let (_, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let sig_int = instantiate(&mut t, &mut o, &mut ctxt, sig, vec![int]);
    assert_eq!(sig_int.kind(&t), TypeKind::Signature);

    // Walk into params/results to confirm T was replaced by int.
    let new_params = signature_params(&t, sig_int).expect("params");
    let p0 = guff_types::tuple_at(&t, new_params, 0);
    assert_eq!(p0.typ(&o), Some(int));
    let new_results = signature_results(&t, sig_int).expect("results");
    let r0 = guff_types::tuple_at(&t, new_results, 0);
    assert_eq!(r0.typ(&o), Some(int));
}

// ----------------------------------------------------------------------------
// Identity of instantiated Named

#[test]
fn identical_treats_same_origin_and_targs_as_equal() {
    let mut t = init_universe().0;
    let mut o = ObjectArena::new();
    let mut ctxt = Context::new();
    let p_arena = PackageArena::new();

    let tn_t = new_type_name(&mut o, "T", None);
    let tp = new_type_param(&mut t, tn_t, None);
    let tlist = bind_tparams(&mut t, vec![tp]).unwrap();

    let slice_of_t = new_slice(&mut t, tp);
    let tn_vec = new_type_name(&mut o, "Vec", None);
    let vec_named = new_named(&mut t, &mut o, tn_vec, Some(slice_of_t), vec![]);
    named_set_type_params(&mut t, vec_named, tlist);

    let (_, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let v1 = instantiate(&mut t, &mut o, &mut ctxt, vec_named, vec![int]);
    let v2 = instantiate(&mut t, &mut o, &mut ctxt, vec_named, vec![int]);
    // Context dedup → same TypeId, so trivially identical.
    assert_eq!(v1, v2);
    assert!(identical(&mut t, &o, &p_arena, v1, v2));

    // Even without dedup, structurally-equal instances are identical.
    let mut ctxt2 = Context::new();
    let v3 = instantiate(&mut t, &mut o, &mut ctxt2, vec_named, vec![int]);
    assert!(identical(&mut t, &o, &p_arena, v1, v3));
}

#[test]
fn identical_treats_different_targs_as_distinct() {
    let mut t = init_universe().0;
    let mut o = ObjectArena::new();
    let mut ctxt = Context::new();
    let p_arena = PackageArena::new();

    let tn_t = new_type_name(&mut o, "T", None);
    let tp = new_type_param(&mut t, tn_t, None);
    let tlist = bind_tparams(&mut t, vec![tp]).unwrap();

    let slice_of_t = new_slice(&mut t, tp);
    let tn_vec = new_type_name(&mut o, "Vec", None);
    let vec_named = new_named(&mut t, &mut o, tn_vec, Some(slice_of_t), vec![]);
    named_set_type_params(&mut t, vec_named, tlist);

    let (_, table) = init_universe();
    let int = table[BasicKind::Int as usize];
    let str_typ = table[BasicKind::String as usize];

    let v_int = instantiate(&mut t, &mut o, &mut ctxt, vec_named, vec![int]);
    let v_str = instantiate(&mut t, &mut o, &mut ctxt, vec_named, vec![str_typ]);
    assert!(!identical(&mut t, &o, &p_arena, v_int, v_str));
}

#[test]
fn instantiated_named_method_list_is_empty_chunk9_deferral() {
    // An instance does NOT store its own expanded method list. Method
    // resolution on instances is done lazily by the Checker at selection time
    // (chunk 67 / D05): `named_lookup_method` searches the origin's methods and
    // `Checker::method_sig_for_recv` substitutes the instance's type arguments
    // into the selected method's signature. So the instance's stored method
    // list is intentionally empty — this pins that (matching Go, which never
    // mutates the origin's list and expands copies on demand).
    let mut t = init_universe().0;
    let mut o = ObjectArena::new();
    let mut ctxt = Context::new();

    let tn_t = new_type_name(&mut o, "T", None);
    let tp = new_type_param(&mut t, tn_t, None);
    let tlist = bind_tparams(&mut t, vec![tp]).unwrap();

    let underlying = new_slice(&mut t, tp);
    let tn_l = new_type_name(&mut o, "List", None);
    let list_named = new_named(&mut t, &mut o, tn_l, Some(underlying), vec![]);
    named_set_type_params(&mut t, list_named, tlist);

    // Origin has a method.
    let sig = new_signature_type(&mut t, None, &[], &[], None, None, false);
    let m = new_func(&mut o, "Len", Some(sig));
    add_method(&mut t, &o, list_named, m);
    assert_eq!(guff_types::named_num_methods(&t, list_named), 1);

    let (_, table) = init_universe();
    let int = table[BasicKind::Int as usize];
    let list_int = instantiate(&mut t, &mut o, &mut ctxt, list_named, vec![int]);
    // Instance currently has 0 methods (deferred to chunk 10).
    assert_eq!(guff_types::named_num_methods(&t, list_int), 0);
}

#[test]
fn nested_subst_through_pointer_and_slice() {
    // smap: T → int. Type: *[]T → *[]int.
    let mut t = init_universe().0;
    let mut o = ObjectArena::new();
    let mut ctxt = Context::new();

    let tn = new_type_name(&mut o, "T", None);
    let tp = new_type_param(&mut t, tn, None);
    let slice_t = new_slice(&mut t, tp);
    let ptr_slice_t = guff_types::new_pointer(&mut t, slice_t);

    let (_, table) = init_universe();
    let int = table[BasicKind::Int as usize];
    let smap = make_subst_map(&[tp], &[int]);
    let result = guff_types::subst(&mut t, &mut o, &smap, None, &mut ctxt, ptr_slice_t);

    assert_eq!(result.kind(&t), TypeKind::Pointer);
    let inner = guff_types::pointer_elem(&t, result);
    assert_eq!(inner.kind(&t), TypeKind::Slice);
    assert_eq!(guff_types::slice_elem(&t, inner), int);
}

// Suppress unused-import lint for ObjectData/TypeData when no assertions
// touch them directly.
#[allow(dead_code)]
fn _silence_unused(_: ObjectData, _: TypeData) {}
