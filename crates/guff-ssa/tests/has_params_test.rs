//! Free-type-parameter detection tests (Milestone E, chunk E07).
//!
//! Exercises `HasParams::has` (Go: `typeparams.Free.Has`): any occurrence of a
//! type parameter makes a type parameterized; fully concrete types are not, and
//! a recursive type terminates via the cycle-breaking cache.

use guff_ssa::has_params::HasParams;
use guff_types::{
    array::new_array,
    basic::{init_universe, BasicKind},
    map::new_map,
    named_set_type_params, new_field, new_named, new_param,
    object::type_name::new_type_name,
    pointer::new_pointer,
    r#struct::new_struct,
    set_underlying,
    signature::new_signature_type,
    slice::new_slice,
    tuple::new_tuple,
    bind_tparams,
    typeparam::new_type_param,
    ObjectArena,
};

/// A bare type parameter, and composites mentioning it, are parameterized; the
/// concrete counterparts are not.
#[test]
fn test_has_params_simple() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];

    let mut objs = ObjectArena::new();
    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);

    let mut hp = HasParams::default();

    // T, []T, *T, [3]T, map[string]T, and struct{ f T } are parameterized.
    assert!(hp.has(&arena, &objs, tparam));
    let slice_t = new_slice(&mut arena, tparam);
    assert!(hp.has(&arena, &objs, slice_t));
    let ptr_t = new_pointer(&mut arena, tparam);
    assert!(hp.has(&arena, &objs, ptr_t));
    let arr_t = new_array(&mut arena, tparam, 3);
    assert!(hp.has(&arena, &objs, arr_t));
    let string_ty = table[BasicKind::String as usize];
    let map_t = new_map(&mut arena, string_ty, tparam);
    assert!(hp.has(&arena, &objs, map_t));
    let f = new_field(&mut objs, "f", tparam, false);
    let st_t = new_struct(&mut arena, vec![f], vec![String::new()]);
    assert!(hp.has(&arena, &objs, st_t));

    // Concrete types are not parameterized.
    assert!(!hp.has(&arena, &objs, int_ty));
    let slice_int = new_slice(&mut arena, int_ty);
    assert!(!hp.has(&arena, &objs, slice_int));
    let g = new_field(&mut objs, "g", int_ty, false);
    let st_int = new_struct(&mut arena, vec![g], vec![String::new()]);
    assert!(!hp.has(&arena, &objs, st_int));
}

/// A signature's own type parameters are declarations, not uses: only the
/// parameter/result types count.
#[test]
fn test_has_params_signature() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];

    let mut objs = ObjectArena::new();
    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);

    let mut hp = HasParams::default();

    // func(a T) int is parameterized (parameter mentions T).
    let a = new_param(&mut objs, "a", tparam);
    let params = new_tuple(&mut arena, &[a]);
    let r = new_param(&mut objs, "", int_ty);
    let results = new_tuple(&mut arena, &[r]);
    let sig = new_signature_type(&mut arena, None, &[], &[], params, results, false);
    assert!(hp.has(&arena, &objs, sig));

    // func(a int) int is concrete.
    let a2 = new_param(&mut objs, "a", int_ty);
    let params2 = new_tuple(&mut arena, &[a2]);
    let r2 = new_param(&mut objs, "", int_ty);
    let results2 = new_tuple(&mut arena, &[r2]);
    let sig2 = new_signature_type(&mut arena, None, &[], &[], params2, results2, false);
    assert!(!hp.has(&arena, &objs, sig2));
}

/// A recursive named type whose underlying mentions T terminates and is
/// parameterized; the same shape over a concrete field is not.
#[test]
fn test_has_params_recursive_named() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];

    let mut objs = ObjectArena::new();
    let mut hp = HasParams::default();

    // Local generic `type X[S any] struct{ s S; next *X }` — uninstantiated
    // (has a type parameter but no type arguments) is parameterized.
    let s_obj = new_type_name(&mut objs, "S", None);
    let s_param = new_type_param(&mut arena, s_obj, None);
    let s_list = bind_tparams(&mut arena, vec![s_param]).unwrap();
    let x_obj = new_type_name(&mut objs, "X", None);
    let x_named = new_named(&mut arena, &mut objs, x_obj, None, vec![]);
    named_set_type_params(&mut arena, x_named, s_list);
    let s_field = new_field(&mut objs, "s", s_param, false);
    let ptr_x = new_pointer(&mut arena, x_named);
    let next_field = new_field(&mut objs, "next", ptr_x, false);
    let x_underlying = new_struct(
        &mut arena,
        vec![s_field, next_field],
        vec![String::new(), String::new()],
    );
    set_underlying(&mut arena, x_named, x_underlying);
    assert!(hp.has(&arena, &objs, x_named), "uninstantiated generic type is parameterized");

    // A plain recursive `type L struct{ next *L }` (no type params) terminates
    // and is concrete.
    let mut hp2 = HasParams::default();
    let l_obj = new_type_name(&mut objs, "L", None);
    let l_named = new_named(&mut arena, &mut objs, l_obj, None, vec![]);
    let ptr_l = new_pointer(&mut arena, l_named);
    let ln_field = new_field(&mut objs, "next", ptr_l, false);
    let n_field = new_field(&mut objs, "n", int_ty, false);
    let l_underlying = new_struct(
        &mut arena,
        vec![ln_field, n_field],
        vec![String::new(), String::new()],
    );
    set_underlying(&mut arena, l_named, l_underlying);
    assert!(!hp2.has(&arena, &objs, l_named), "concrete recursive type terminates and is not parameterized");
}
