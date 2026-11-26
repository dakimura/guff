//! Chunk-13 tests: `infer.rs` — type-argument inference.

use guff_types::{
    bind_tparams, core_term, infer, init_universe_full, is_parameterized, kill_cycles,
    new_interface_type, new_named, new_param, new_pointer, new_signature_type, new_slice, new_term,
    new_tuple, new_type_name, new_type_param, new_union, rename_tparams, set_constraint, BasicKind,
    InferResult, TypeId,
};

// ----------------------------------------------------------------------------
// isParameterized

#[test]
fn is_parameterized_finds_tparam_in_slice() {
    let mut u = init_universe_full();
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);

    let s = new_slice(&mut u.type_arena, tp);
    assert!(is_parameterized(&u.type_arena, &u.object_arena, &[tp], s));
    // []int doesn't contain T.
    let int = u.typ[BasicKind::Int as usize];
    let si = new_slice(&mut u.type_arena, int);
    assert!(!is_parameterized(&u.type_arena, &u.object_arena, &[tp], si));
}

#[test]
fn is_parameterized_traverses_signature_params_and_results() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);

    // func(T) int
    let p = new_param(&mut u.object_arena, "x", tp);
    let params = new_tuple(&mut u.type_arena, &[p]);
    let r = new_param(&mut u.object_arena, "", int);
    let results = new_tuple(&mut u.type_arena, &[r]);
    let sig = new_signature_type(&mut u.type_arena, None, &[], &[], params, results, false);
    assert!(is_parameterized(&u.type_arena, &u.object_arena, &[tp], sig));

    // func(int) int is not parameterized.
    let p2 = new_param(&mut u.object_arena, "x", int);
    let params2 = new_tuple(&mut u.type_arena, &[p2]);
    let r2 = new_param(&mut u.object_arena, "", int);
    let results2 = new_tuple(&mut u.type_arena, &[r2]);
    let sig2 = new_signature_type(&mut u.type_arena, None, &[], &[], params2, results2, false);
    assert!(!is_parameterized(
        &u.type_arena,
        &u.object_arena,
        &[tp],
        sig2
    ));
}

// ----------------------------------------------------------------------------
// core_term

#[test]
fn core_term_single_term_constraint_returns_single() {
    // type P interface { int }
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let union_t = new_term(false, int);
    let union = new_union(&mut u.type_arena, vec![union_t]);
    let iface = new_interface_type(&mut u.type_arena, vec![], vec![union]);

    let tn = new_type_name(&mut u.object_arena, "P", None);
    let tp = new_type_param(&mut u.type_arena, tn, Some(iface));

    let core = core_term(&mut u.type_arena, &u.object_arena, &u.package_arena, tp)
        .expect("core term exists");
    assert!(core.single);
    assert!(!core.tilde);
    assert_eq!(core.typ, int);
}

#[test]
fn core_term_empty_constraint_returns_none() {
    // type P any  — no specific types.
    let mut u = init_universe_full();
    let tn = new_type_name(&mut u.object_arena, "P", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    assert!(core_term(&mut u.type_arena, &u.object_arena, &u.package_arena, tp,).is_none());
}

// ----------------------------------------------------------------------------
// kill_cycles

#[test]
fn kill_cycles_breaks_self_referential_tparam() {
    // T inferred to *T  — cycle, should be killed.
    let mut u = init_universe_full();
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);
    let ptr_t = new_pointer(&mut u.type_arena, tp);

    let mut inferred: Vec<Option<TypeId>> = vec![Some(ptr_t)];
    kill_cycles(&u.type_arena, &u.object_arena, &[tp], &mut inferred);
    assert_eq!(inferred[0], None);
}

#[test]
fn kill_cycles_preserves_non_cyclic_inference() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);

    let mut inferred = vec![Some(int)];
    kill_cycles(&u.type_arena, &u.object_arena, &[tp], &mut inferred);
    assert_eq!(inferred[0], Some(int));
}

// ----------------------------------------------------------------------------
// rename_tparams

#[test]
fn rename_tparams_creates_fresh_tparams() {
    let mut u = init_universe_full();
    let tn = new_type_name(&mut u.object_arena, "P", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);
    let s = new_slice(&mut u.type_arena, tp);

    let (new_tps, new_typ) = rename_tparams(&mut u.type_arena, &mut u.object_arena, &[tp], s);
    assert_eq!(new_tps.len(), 1);
    // Fresh tparam: different TypeId from the original.
    assert_ne!(new_tps[0], tp);
    // The renamed slice must point at the new tparam, not the old.
    assert!(is_parameterized(
        &u.type_arena,
        &u.object_arena,
        &new_tps,
        new_typ
    ));
    assert!(!is_parameterized(
        &u.type_arena,
        &u.object_arena,
        &[tp],
        new_typ
    ));
}

#[test]
fn rename_tparams_empty_returns_unchanged() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let (new_tps, new_typ) = rename_tparams(&mut u.type_arena, &mut u.object_arena, &[], int);
    assert!(new_tps.is_empty());
    assert_eq!(new_typ, int);
}

// ----------------------------------------------------------------------------
// infer — main entry

#[test]
fn infer_single_tparam_from_arg() {
    // func[T any](x T) — infer from arg int  ⇒  T = int.
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);
    let pvar = new_param(&mut u.object_arena, "x", tp);
    let params = new_tuple(&mut u.type_arena, &[pvar]);

    let res = infer(
        &mut u.type_arena,
        &mut u.object_arena,
        &u.package_arena,
        &[tp],
        &[],
        params,
        &[Some(int)],
        &[],
        &u.typ,
        false,
    );
    match res {
        InferResult::Ok(targs) => assert_eq!(targs, vec![int]),
        InferResult::Failed(_) => panic!("expected Ok"),
    }
}

#[test]
fn infer_two_tparams_from_two_args() {
    // func[T,U any](x T, y U) — args (int, string) ⇒ T=int, U=string.
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let s = u.typ[BasicKind::String as usize];
    let tn_t = new_type_name(&mut u.object_arena, "T", None);
    let tn_v = new_type_name(&mut u.object_arena, "U", None);
    let tp_t = new_type_param(&mut u.type_arena, tn_t, None);
    let tp_u = new_type_param(&mut u.type_arena, tn_v, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp_t, tp_u]);

    let p_t = new_param(&mut u.object_arena, "x", tp_t);
    let p_u = new_param(&mut u.object_arena, "y", tp_u);
    let params = new_tuple(&mut u.type_arena, &[p_t, p_u]);

    let res = infer(
        &mut u.type_arena,
        &mut u.object_arena,
        &u.package_arena,
        &[tp_t, tp_u],
        &[],
        params,
        &[Some(int), Some(s)],
        &[],
        &u.typ,
        false,
    );
    match res {
        InferResult::Ok(targs) => assert_eq!(targs, vec![int, s]),
        InferResult::Failed(_) => panic!("expected Ok"),
    }
}

#[test]
fn infer_through_slice_param() {
    // func[T any](xs []T) — arg []int ⇒ T = int.
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);
    let slice_t = new_slice(&mut u.type_arena, tp);
    let pvar = new_param(&mut u.object_arena, "xs", slice_t);
    let params = new_tuple(&mut u.type_arena, &[pvar]);
    let slice_int = new_slice(&mut u.type_arena, int);

    let res = infer(
        &mut u.type_arena,
        &mut u.object_arena,
        &u.package_arena,
        &[tp],
        &[],
        params,
        &[Some(slice_int)],
        &[],
        &u.typ,
        false,
    );
    match res {
        InferResult::Ok(targs) => assert_eq!(targs, vec![int]),
        InferResult::Failed(_) => panic!("expected Ok"),
    }
}

#[test]
fn infer_fails_when_args_conflict() {
    // func[T any](x, y T) — args (int, string) ⇒ failure.
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let s = u.typ[BasicKind::String as usize];
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);
    let p1 = new_param(&mut u.object_arena, "x", tp);
    let p2 = new_param(&mut u.object_arena, "y", tp);
    let params = new_tuple(&mut u.type_arena, &[p1, p2]);

    let res = infer(
        &mut u.type_arena,
        &mut u.object_arena,
        &u.package_arena,
        &[tp],
        &[],
        params,
        &[Some(int), Some(s)],
        &[],
        &u.typ,
        false,
    );
    assert!(matches!(res, InferResult::Failed(_)));
}

#[test]
fn infer_pre_bound_targ_substituted_skipped_when_non_parameterized() {
    // func[T,U any](x T, y U) with T pre-bound to int, args (string,string).
    //
    // Per Go's design: substituting T → int into params makes the first
    // param become `int` (non-parameterized). Step 1 of infer skips any
    // (par, arg) pair where neither side is parameterized in tparams.
    // This gives BETTER error messages later (in operand/assignments) —
    // the inference reports T=int, U=string. The actual int-vs-string
    // mismatch is reported by assignment checking, not by infer.
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let s = u.typ[BasicKind::String as usize];
    let tn_t = new_type_name(&mut u.object_arena, "T", None);
    let tn_v = new_type_name(&mut u.object_arena, "U", None);
    let tp_t = new_type_param(&mut u.type_arena, tn_t, None);
    let tp_u = new_type_param(&mut u.type_arena, tn_v, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp_t, tp_u]);
    let p_t = new_param(&mut u.object_arena, "x", tp_t);
    let p_u = new_param(&mut u.object_arena, "y", tp_u);
    let params = new_tuple(&mut u.type_arena, &[p_t, p_u]);

    let res = infer(
        &mut u.type_arena,
        &mut u.object_arena,
        &u.package_arena,
        &[tp_t, tp_u],
        &[Some(int)],
        params,
        &[Some(s), Some(s)],
        &[],
        &u.typ,
        false,
    );
    match res {
        InferResult::Ok(targs) => assert_eq!(targs, vec![int, s]),
        InferResult::Failed(_) => {
            panic!("expected Ok — infer leaves int/string mismatch to assignment checking")
        }
    }
}

#[test]
fn infer_fast_path_when_all_targs_provided() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let s = u.typ[BasicKind::String as usize];
    let tn_t = new_type_name(&mut u.object_arena, "T", None);
    let tn_v = new_type_name(&mut u.object_arena, "U", None);
    let tp_t = new_type_param(&mut u.type_arena, tn_t, None);
    let tp_u = new_type_param(&mut u.type_arena, tn_v, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp_t, tp_u]);

    let res = infer(
        &mut u.type_arena,
        &mut u.object_arena,
        &u.package_arena,
        &[tp_t, tp_u],
        &[Some(int), Some(s)],
        None,
        &[],
        &[],
        &u.typ,
        false,
    );
    match res {
        InferResult::Ok(targs) => assert_eq!(targs, vec![int, s]),
        InferResult::Failed(_) => panic!("expected Ok fast path"),
    }
}

#[test]
fn infer_constraint_with_single_term_supplies_unknown() {
    // type T interface { int }; func[T constraint]()  with no args
    // ⇒ T = int from the constraint's single term.
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let union_t = new_term(false, int);
    let union = new_union(&mut u.type_arena, vec![union_t]);
    let iface = new_interface_type(&mut u.type_arena, vec![], vec![union]);
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);
    set_constraint(&mut u.type_arena, tp, iface);

    let res = infer(
        &mut u.type_arena,
        &mut u.object_arena,
        &u.package_arena,
        &[tp],
        &[],
        None,
        &[],
        &[],
        &u.typ,
        false,
    );
    match res {
        InferResult::Ok(targs) => assert_eq!(targs, vec![int]),
        InferResult::Failed(_) => panic!("expected Ok"),
    }
}

#[test]
fn infer_untyped_constant_defaults_to_int() {
    // func[T any](x T) — withheld untyped-int arg defaults T to int (step 3).
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let untyped_int = u.typ[BasicKind::UntypedInt as usize];
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);
    let pvar = new_param(&mut u.object_arena, "x", tp);
    let params = new_tuple(&mut u.type_arena, &[pvar]);

    let res = infer(
        &mut u.type_arena,
        &mut u.object_arena,
        &u.package_arena,
        &[tp],
        &[],
        params,
        &[None],              // untyped arg withheld from step 1
        &[Some(untyped_int)], // supplied for step 3 defaulting
        &u.typ,
        false,
    );
    match res {
        InferResult::Ok(targs) => assert_eq!(targs, vec![int]),
        InferResult::Failed(_) => panic!("expected Ok"),
    }
}

#[test]
fn infer_two_untyped_constants_take_max_type() {
    // func[T any](a, b T) — untyped int and untyped float ⇒ T defaults to the
    // maximum untyped type's default (float64), not a unification conflict.
    let mut u = init_universe_full();
    let f64 = u.typ[BasicKind::Float64 as usize];
    let untyped_int = u.typ[BasicKind::UntypedInt as usize];
    let untyped_float = u.typ[BasicKind::UntypedFloat as usize];
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let _ = bind_tparams(&mut u.type_arena, vec![tp]);
    let a = new_param(&mut u.object_arena, "a", tp);
    let b = new_param(&mut u.object_arena, "b", tp);
    let params = new_tuple(&mut u.type_arena, &[a, b]);

    let res = infer(
        &mut u.type_arena,
        &mut u.object_arena,
        &u.package_arena,
        &[tp],
        &[],
        params,
        &[None, None],
        &[Some(untyped_int), Some(untyped_float)],
        &u.typ,
        false,
    );
    match res {
        InferResult::Ok(targs) => assert_eq!(targs, vec![f64]),
        InferResult::Failed(_) => panic!("expected Ok"),
    }
}

// Silence unused imports.
#[test]
fn unused_imports_smoke() {
    let _ = new_named;
}
