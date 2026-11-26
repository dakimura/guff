//! Type-substitution tests (Milestone E, chunks E01–E03).
//!
//! Exercises the `Subster` over the leaf, pure-`TypeId` composite, and
//! object-bearing (`Tuple`/`Struct`/`Signature`) type kinds: a substituted type
//! parameter maps to its replacement, and composite types are rebuilt only when
//! a contained type actually changes (otherwise the same `TypeId` is preserved).

use guff_ssa::subst::Subster;
use guff_types::{
    array::{array_elem, array_len, new_array},
    basic::{init_universe, BasicKind},
    chan::{chan_dir, chan_elem, new_chan, ChanDir},
    map::{map_elem, map_key, new_map},
    object::type_name::new_type_name,
    pointer::{new_pointer, pointer_elem},
    slice::{new_slice, slice_elem},
    typeparam::new_type_param,
    union::{new_term, new_union, union_len, union_term},
    ObjectArena,
};

/// A single type parameter `T` substituted with `int` rewrites `[]T`, `*T`, and
/// `[3]T` to `[]int`, `*int`, and `[3]int`, and leaves an unrelated `string`
/// type (and the reused `int`) untouched.
#[test]
fn test_subst_simple_composites() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let string_ty = table[BasicKind::String as usize];

    // A bare type parameter T (constraint elided — irrelevant to substitution).
    let mut objs = ObjectArena::new();
    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);

    // Composite types mentioning T.
    let slice_t = new_slice(&mut arena, tparam); // []T
    let ptr_t = new_pointer(&mut arena, tparam); // *T
    let arr_t = new_array(&mut arena, tparam, 3); // [3]T

    let mut subst = Subster::new(&[tparam], &[int_ty]);
    assert!(!subst.is_identity());

    // T -> int
    assert_eq!(subst.typ(&mut arena, &mut objs, tparam), int_ty);

    // []T -> []int (a freshly built slice whose element is int).
    let slice_int = subst.typ(&mut arena, &mut objs, slice_t);
    assert_ne!(slice_int, slice_t, "a changed slice is rebuilt");
    assert_eq!(slice_elem(&arena, slice_int), int_ty);

    // *T -> *int
    let ptr_int = subst.typ(&mut arena, &mut objs, ptr_t);
    assert_ne!(ptr_int, ptr_t);
    assert_eq!(pointer_elem(&arena, ptr_int), int_ty);

    // [3]T -> [3]int (length preserved).
    let arr_int = subst.typ(&mut arena, &mut objs, arr_t);
    assert_ne!(arr_int, arr_t);
    assert_eq!(array_elem(&arena, arr_int), int_ty);
    assert_eq!(array_len(&arena, arr_int), 3);

    // A type with no substituted parameter is preserved by identity.
    assert_eq!(subst.typ(&mut arena, &mut objs, string_ty), string_ty);
    let slice_string = new_slice(&mut arena, string_ty);
    assert_eq!(
        subst.typ(&mut arena, &mut objs, slice_string),
        slice_string,
        "an unchanged composite keeps its TypeId"
    );
}

/// Substitution recurses into the pure-`TypeId` composite kinds `Map`, `Chan`,
/// and `Union` (chunk E02), rebuilding each only when a contained type changes.
#[test]
fn test_subst_map_chan_union() {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let string_ty = table[BasicKind::String as usize];

    let mut objs = ObjectArena::new();
    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);

    // map[string]T -> map[string]int (key unchanged, elem substituted).
    let map_t = new_map(&mut arena, string_ty, tparam);
    let mut subst = Subster::new(&[tparam], &[int_ty]);
    let map_int = subst.typ(&mut arena, &mut objs, map_t);
    assert_ne!(map_int, map_t, "a changed map is rebuilt");
    assert_eq!(map_key(&arena, map_int), string_ty);
    assert_eq!(map_elem(&arena, map_int), int_ty);

    // chan<-T -> chan<-int (direction preserved).
    let chan_t = new_chan(&mut arena, ChanDir::RecvOnly, tparam);
    let chan_int = subst.typ(&mut arena, &mut objs, chan_t);
    assert_ne!(chan_int, chan_t);
    assert_eq!(chan_dir(&arena, chan_int), ChanDir::RecvOnly);
    assert_eq!(chan_elem(&arena, chan_int), int_ty);

    // ~T | string -> ~int | string (tilde of each term preserved).
    let union_t = new_union(
        &mut arena,
        vec![new_term(true, tparam), new_term(false, string_ty)],
    );
    let union_int = subst.typ(&mut arena, &mut objs, union_t);
    assert_ne!(union_int, union_t);
    assert_eq!(union_len(&arena, union_int), 2);
    let term0 = union_term(&arena, union_int, 0);
    assert!(term0.tilde());
    assert_eq!(term0.typ(), int_ty);
    let term1 = union_term(&arena, union_int, 1);
    assert!(!term1.tilde());
    assert_eq!(term1.typ(), string_ty);

    // Composites with no substituted parameter are preserved by identity.
    let map_ss = new_map(&mut arena, string_ty, string_ty);
    assert_eq!(subst.typ(&mut arena, &mut objs, map_ss), map_ss);
    let chan_s = new_chan(&mut arena, ChanDir::SendRecv, string_ty);
    assert_eq!(subst.typ(&mut arena, &mut objs, chan_s), chan_s);
    let union_s = new_union(&mut arena, vec![new_term(false, string_ty)]);
    assert_eq!(subst.typ(&mut arena, &mut objs, union_s), union_s);
}

/// Substitution recurses into the object-bearing kinds `Tuple`, `Struct`, and
/// `Signature` (chunk E03), rewriting each contained variable's type and
/// rebuilding only when something actually changed.
#[test]
fn test_subst_tuple_struct_signature() {
    use guff_types::{
        new_field, new_param,
        signature::{
            new_signature_type, signature_params, signature_recv, signature_results,
            signature_variadic,
        },
        r#struct::{new_struct, struct_field, struct_num_fields, struct_tag},
        tuple::{new_tuple, tuple_at, tuple_len},
        ObjectArena,
    };
    use guff_types::object::var::Var;
    use guff_types::ObjectData;

    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let string_ty = table[BasicKind::String as usize];

    let mut objs = ObjectArena::new();
    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);

    let field_typ = |objs: &ObjectArena, id| match objs.get(id) {
        ObjectData::Var(v) => v.typ(),
        _ => panic!("expected Var"),
    };
    let is_field = |objs: &ObjectArena, id| match objs.get(id) {
        ObjectData::Var(v) => Var::is_field(v),
        _ => panic!("expected Var"),
    };

    let mut subst = Subster::new(&[tparam], &[int_ty]);

    // Tuple (x T, y string) -> (x int, y string); only the changed var is fresh.
    let x = new_param(&mut objs, "x", tparam);
    let y = new_param(&mut objs, "y", string_ty);
    let tup = new_tuple(&mut arena, &[x, y]).unwrap();
    let tup2 = subst.typ(&mut arena, &mut objs, tup);
    assert_ne!(tup2, tup, "a changed tuple is rebuilt");
    assert_eq!(tuple_len(&arena, Some(tup2)), 2);
    let x2 = tuple_at(&arena, tup2, 0);
    let y2 = tuple_at(&arena, tup2, 1);
    assert_ne!(x2, x, "the substituted element is a fresh Var");
    assert_eq!(field_typ(&objs, x2), int_ty);
    assert_eq!(y2, y, "the unchanged element keeps its ObjectId");

    // Struct with a field of type T: field type substituted, tag preserved,
    // embedded flag preserved.
    let f = new_field(&mut objs, "F", tparam, false);
    let st = new_struct(&mut arena, vec![f], vec!["json:\"f\"".to_string()]);
    let st2 = subst.typ(&mut arena, &mut objs, st);
    assert_ne!(st2, st);
    assert_eq!(struct_num_fields(&arena, st2), 1);
    let f2 = struct_field(&arena, st2, 0);
    assert!(is_field(&objs, f2), "a substituted field stays a field");
    assert_eq!(field_typ(&objs, f2), int_ty);
    assert_eq!(struct_tag(&arena, st2, 0), "json:\"f\"");

    // Signature func(a T) T: params tuple and results tuple both substituted.
    let a = new_param(&mut objs, "a", tparam);
    let params = new_tuple(&mut arena, &[a]);
    let r = new_param(&mut objs, "", tparam);
    let results = new_tuple(&mut arena, &[r]);
    let sig = new_signature_type(&mut arena, None, &[], &[], params, results, false);
    let sig2 = subst.typ(&mut arena, &mut objs, sig);
    assert_ne!(sig2, sig);
    assert_eq!(signature_recv(&arena, sig2), None);
    assert!(!signature_variadic(&arena, sig2));
    let p2 = signature_params(&arena, sig2).expect("params tuple");
    assert_eq!(field_typ(&objs, tuple_at(&arena, p2, 0)), int_ty);
    let res2 = signature_results(&arena, sig2).expect("results tuple");
    assert_eq!(field_typ(&objs, tuple_at(&arena, res2, 0)), int_ty);

    // A struct with no substituted parameter is preserved by identity.
    let g = new_field(&mut objs, "G", string_ty, false);
    let st_s = new_struct(&mut arena, vec![g], vec![String::new()]);
    assert_eq!(subst.typ(&mut arena, &mut objs, st_s), st_s);
}

/// Substitution over `Named` and `Alias` types declared outside the origin
/// (chunk E04): a plain (non-instance) named type is preserved, while an
/// instance `Orig[T]` has its type arguments substituted and is re-instantiated
/// to `Orig[int]`.
#[test]
fn test_subst_named_alias_instance() {
    use guff_types::{
        alias_set_type_params, bind_tparams, instantiate, named_set_type_params, named_type_args,
        new_alias, new_named, Context, ObjectArena, TypeKind,
    };

    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];

    let mut objs = ObjectArena::new();
    // A separate context used only to build the *input* instances under test.
    let mut ctxt = Context::new();

    // The substituter's own type parameter T, replaced with int.
    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);
    let mut subst = Subster::new(&[tparam], &[int_ty]);

    // ----- Non-generic named type MyInt: preserved unchanged. -----
    let myint_obj = new_type_name(&mut objs, "MyInt", None);
    let myint = new_named(&mut arena, &mut objs, myint_obj, Some(int_ty), vec![]);
    assert_eq!(
        subst.typ(&mut arena, &mut objs, myint),
        myint,
        "a non-generic named type is type-parameter-free and preserved"
    );

    // ----- Generic named type Vec[E any] = []E, instantiated with T. -----
    let e_obj = new_type_name(&mut objs, "E", None);
    let e_param = new_type_param(&mut arena, e_obj, None);
    let e_list = bind_tparams(&mut arena, vec![e_param]).unwrap();
    let vec_obj = new_type_name(&mut objs, "Vec", None);
    let slice_of_e = new_slice(&mut arena, e_param);
    let vec_named = new_named(&mut arena, &mut objs, vec_obj, Some(slice_of_e), vec![]);
    named_set_type_params(&mut arena, vec_named, e_list);

    // Build the instance Vec[T] to feed into substitution.
    let vec_t = instantiate(&mut arena, &mut objs, &mut ctxt, vec_named, vec![tparam]);
    // Substituting T := int turns Vec[T] into Vec[int].
    let vec_int = subst.typ(&mut arena, &mut objs, vec_t);
    assert_ne!(vec_int, vec_t, "a generic instance is re-instantiated");
    assert_eq!(vec_int.kind(&arena), TypeKind::Named);
    let targs = named_type_args(&arena, vec_int).expect("Vec[int] has type args");
    assert_eq!(targs.len(), 1);
    assert_eq!(targs.at(0), int_ty, "the type argument was substituted to int");
    // Idempotent via the result cache / instantiation dedup.
    assert_eq!(subst.typ(&mut arena, &mut objs, vec_t), vec_int);

    // ----- Non-generic alias preserved; generic alias instance re-instantiated.
    let ai_obj = new_type_name(&mut objs, "AInt", None);
    let aint = new_alias(&mut arena, &mut objs, ai_obj, Some(int_ty));
    assert_eq!(subst.typ(&mut arena, &mut objs, aint), aint);

    let ae_obj = new_type_name(&mut objs, "AE", None);
    let ae_param = new_type_param(&mut arena, ae_obj, None);
    let ae_list = bind_tparams(&mut arena, vec![ae_param]).unwrap();
    let lst_obj = new_type_name(&mut objs, "Lst", None);
    let slice_of_ae = new_slice(&mut arena, ae_param);
    let lst_alias = new_alias(&mut arena, &mut objs, lst_obj, Some(slice_of_ae));
    alias_set_type_params(&mut arena, lst_alias, ae_list);

    let lst_t = instantiate(&mut arena, &mut objs, &mut ctxt, lst_alias, vec![tparam]);
    let lst_int = subst.typ(&mut arena, &mut objs, lst_t);
    assert_ne!(lst_int, lst_t, "a generic alias instance is re-instantiated");
}

/// Substitution over an `Interface` (chunk E05): a method whose signature
/// mentions the type parameter is rewritten (via its receiver-less signature),
/// while an interface with no substituted parameter is preserved.
#[test]
fn test_subst_interface() {
    use guff_types::{
        interface_explicit_method, interface_num_explicit_methods, new_func, new_interface_type,
        new_param, new_signature_type, new_tuple, signature_results, tuple_at, ObjectArena,
        ObjectData, TypeKind,
    };

    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let string_ty = table[BasicKind::String as usize];

    let mut objs = ObjectArena::new();
    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);
    let mut subst = Subster::new(&[tparam], &[int_ty]);

    let method_returning = |arena: &mut _, objs: &mut ObjectArena, name: &str, ret: _| {
        let results = new_tuple(arena, &[new_param(objs, "", ret)]);
        let sig = new_signature_type(arena, None, &[], &[], None, results, false);
        new_func(objs, name, Some(sig))
    };
    let result_type = |arena: &_, objs: &ObjectArena, iface, i| {
        let f = interface_explicit_method(arena, iface, i);
        let sig = match objs.get(f) {
            ObjectData::Func(func) => func.typ().unwrap(),
            _ => panic!("expected Func"),
        };
        let results = signature_results(arena, sig).expect("has results");
        let r = tuple_at(arena, results, 0);
        match objs.get(r) {
            ObjectData::Var(v) => v.typ(),
            _ => panic!("expected Var"),
        }
    };

    // interface { Get() T } -> interface { Get() int }.
    let get = method_returning(&mut arena, &mut objs, "Get", tparam);
    let iface_t = new_interface_type(&mut arena, vec![get], vec![]);
    let iface_int = subst.typ(&mut arena, &mut objs, iface_t);
    assert_ne!(iface_int, iface_t, "a changed interface is rebuilt");
    assert_eq!(iface_int.kind(&arena), TypeKind::Interface);
    assert_eq!(interface_num_explicit_methods(&arena, iface_int), 1);
    assert_eq!(result_type(&arena, &objs, iface_int, 0), int_ty);

    // interface { Name() string } has no T -> preserved.
    let name = method_returning(&mut arena, &mut objs, "Name", string_ty);
    let iface_s = new_interface_type(&mut arena, vec![name], vec![]);
    assert_eq!(
        subst.typ(&mut arena, &mut objs, iface_s),
        iface_s,
        "an interface free of the type parameter keeps its TypeId"
    );
}

/// Substitution over a `Named` type declared *within* the origin function
/// (chunk E06): a local type is copied fresh per instantiation with the
/// substitution applied to its underlying type, its self-references rewired to
/// the copy (so a recursive `type X struct{ s T; next *X }` terminates), while a
/// package-level type (declared outside the origin's scope) is still preserved.
#[test]
fn test_subst_local_named() {
    use guff_types::{
        named_underlying, new_named, pointer::new_pointer, pointer::pointer_elem, set_underlying,
        r#struct::{new_struct, struct_field, struct_num_fields},
        new_field, ObjectData, TypeKind,
    };

    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];

    let mut objs = ObjectArena::new();
    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);

    // Origin function `F[T]` occupies source range [100, 200).
    let mut subst = Subster::new(&[tparam], &[int_ty]).in_origin(100, 200);

    // Recursive local type `type X struct{ s T; next *X }` declared at pos 150.
    let x_obj = new_type_name(&mut objs, "X", None);
    x_obj.set_pos(&mut objs, 150);
    let x_named = new_named(&mut arena, &mut objs, x_obj, None, vec![]); // incomplete
    let s_field = new_field(&mut objs, "s", tparam, false);
    let ptr_x = new_pointer(&mut arena, x_named);
    let next_field = new_field(&mut objs, "next", ptr_x, false);
    let x_underlying = new_struct(
        &mut arena,
        vec![s_field, next_field],
        vec![String::new(), String::new()],
    );
    set_underlying(&mut arena, x_named, x_underlying);

    // Substituting T := int makes a fresh copy X' with underlying
    // struct{ s int; next *X' } — the self-reference points at the copy.
    let x_int = subst.typ(&mut arena, &mut objs, x_named);
    assert_ne!(x_int, x_named, "a local type is copied fresh per instantiation");
    assert_eq!(x_int.kind(&arena), TypeKind::Named);

    let u = named_underlying(&arena, x_int).expect("copy has an underlying");
    assert_eq!(struct_num_fields(&arena, u), 2);
    let f0 = struct_field(&arena, u, 0);
    let f0_typ = match objs.get(f0) {
        ObjectData::Var(v) => v.typ(),
        _ => panic!("expected Var"),
    };
    assert_eq!(f0_typ, int_ty, "field `s` was substituted to int");
    let f1 = struct_field(&arena, u, 1);
    let f1_typ = match objs.get(f1) {
        ObjectData::Var(v) => v.typ(),
        _ => panic!("expected Var"),
    };
    assert_eq!(f1_typ.kind(&arena), TypeKind::Pointer);
    assert_eq!(
        pointer_elem(&arena, f1_typ),
        x_int,
        "the recursive self-reference resolves to the fresh copy"
    );
    // Idempotent via the result cache.
    assert_eq!(subst.typ(&mut arena, &mut objs, x_named), x_int);

    // A package-level named type (declared outside the origin scope, pos 50) is
    // type-parameter-free and preserved even while an origin is set.
    let out_obj = new_type_name(&mut objs, "Out", None);
    out_obj.set_pos(&mut objs, 50);
    let out_named = new_named(&mut arena, &mut objs, out_obj, Some(int_ty), vec![]);
    assert_eq!(
        subst.typ(&mut arena, &mut objs, out_named),
        out_named,
        "a type declared outside the origin is preserved"
    );
}

/// Substitution over a generic `Named` type declared within the origin (chunk
/// E06): the copy gets fresh type parameters (so its own parameter is distinct
/// from the original), occurrences of the copy's parameter in the underlying map
/// to the copy, the origin's parameter is substituted, and the fresh parameter's
/// constraint is substituted too.
#[test]
fn test_subst_local_generic_named() {
    use guff_types::{
        named_underlying, new_named, named_set_type_params, bind_tparams,
        r#struct::{new_struct, struct_field, struct_num_fields},
        new_field, ObjectData, TypeData, TypeKind,
    };

    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];

    let mut objs = ObjectArena::new();
    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);

    let mut subst = Subster::new(&[tparam], &[int_ty]).in_origin(100, 200);

    // Local generic type `type P[S []T] struct{ f S; t T }` at pos 150.
    let p_obj = new_type_name(&mut objs, "P", None);
    p_obj.set_pos(&mut objs, 150);
    let s_obj = new_type_name(&mut objs, "S", None);
    s_obj.set_pos(&mut objs, 160);
    // Constraint of S mentions T (a []T), so we can observe it being substituted.
    let slice_t = new_slice(&mut arena, tparam);
    let s_param = new_type_param(&mut arena, s_obj, Some(slice_t));
    let s_list = bind_tparams(&mut arena, vec![s_param]).unwrap();

    let p_named = new_named(&mut arena, &mut objs, p_obj, None, vec![]);
    named_set_type_params(&mut arena, p_named, s_list);

    let f_field = new_field(&mut objs, "f", s_param, false); // f S
    let t_field = new_field(&mut objs, "t", tparam, false); // t T
    let p_underlying = new_struct(
        &mut arena,
        vec![f_field, t_field],
        vec![String::new(), String::new()],
    );
    guff_types::set_underlying(&mut arena, p_named, p_underlying);

    let p_int = subst.typ(&mut arena, &mut objs, p_named);
    assert_ne!(p_int, p_named);
    assert_eq!(p_int.kind(&arena), TypeKind::Named);

    // The copy has a fresh type parameter distinct from the original S.
    let copy_tparams: Vec<_> = match arena.get(p_int) {
        TypeData::Named(n) => n.type_params().map(|l| l.list().to_vec()).unwrap_or_default(),
        _ => panic!("expected Named"),
    };
    assert_eq!(copy_tparams.len(), 1);
    let s_copy = copy_tparams[0];
    assert_ne!(s_copy, s_param, "the copy has its own fresh type parameter");

    let u = named_underlying(&arena, p_int).unwrap();
    assert_eq!(struct_num_fields(&arena, u), 2);
    // f: the copy's own parameter (not the original S, not int).
    let f0 = struct_field(&arena, u, 0);
    let f0_typ = match objs.get(f0) {
        ObjectData::Var(v) => v.typ(),
        _ => panic!("expected Var"),
    };
    assert_eq!(f0_typ, s_copy, "field `f` maps to the copy's parameter");
    // t: the origin's parameter T, substituted to int.
    let f1 = struct_field(&arena, u, 1);
    let f1_typ = match objs.get(f1) {
        ObjectData::Var(v) => v.typ(),
        _ => panic!("expected Var"),
    };
    assert_eq!(f1_typ, int_ty, "field `t` was substituted to int");

    // The copy's parameter constraint []T was substituted to []int.
    let s_copy_bound =
        guff_types::type_param_constraint(&arena, s_copy).expect("copy param has a constraint");
    assert_eq!(s_copy_bound.kind(&arena), TypeKind::Slice);
    assert_eq!(guff_types::slice::slice_elem(&arena, s_copy_bound), int_ty);
}

/// Substitution over an `Alias` declared within the origin (chunk E06): a local
/// alias `type B = []T` is copied fresh with its right-hand side substituted.
#[test]
fn test_subst_local_alias() {
    use guff_types::{alias_rhs, new_alias, slice::slice_elem, TypeKind};

    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];

    let mut objs = ObjectArena::new();
    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);

    let mut subst = Subster::new(&[tparam], &[int_ty]).in_origin(100, 200);

    // Local alias `type B = []T` declared at pos 150.
    let b_obj = new_type_name(&mut objs, "B", None);
    b_obj.set_pos(&mut objs, 150);
    let slice_t = new_slice(&mut arena, tparam);
    let b_alias = new_alias(&mut arena, &mut objs, b_obj, Some(slice_t));

    let b_int = subst.typ(&mut arena, &mut objs, b_alias);
    assert_ne!(b_int, b_alias, "a local alias is copied fresh");
    assert_eq!(b_int.kind(&arena), TypeKind::Alias);
    let rhs = alias_rhs(&arena, b_int).expect("copy has a right-hand side");
    assert_eq!(rhs.kind(&arena), TypeKind::Slice);
    assert_eq!(slice_elem(&arena, rhs), int_ty, "rhs []T substituted to []int");
}

/// Receiver and function type parameters are substituted independently.
#[test]
fn test_subst_with_recv_and_type() {
    let (mut arena, table) = init_universe();
    let mut oarena = ObjectArena::new();
    let int_ty = table[BasicKind::Int as usize];
    let string_ty = table[BasicKind::String as usize];

    let s_obj = new_type_name(&mut oarena, "S", None);
    let t_obj = new_type_name(&mut oarena, "T", None);
    let s = new_type_param(&mut arena, s_obj, None);
    let t = new_type_param(&mut arena, t_obj, None);

    let mut subst = Subster::with_recv_and_type(&[s], &[int_ty], &[t], &[string_ty]);
    let slice_s = new_slice(&mut arena, s);
    let slice_t = new_slice(&mut arena, t);
    let out_s = subst.typ(&mut arena, &mut oarena, slice_s);
    let out_t = subst.typ(&mut arena, &mut oarena, slice_t);
    assert_eq!(slice_elem(&arena, out_s), int_ty);
    assert_eq!(slice_elem(&arena, out_t), string_ty);
}

/// An empty substitution is the identity on every type.
#[test]
fn test_subst_identity() {
    let (mut arena, table) = init_universe();
    let mut objs = ObjectArena::new();
    let int_ty = table[BasicKind::Int as usize];
    let slice_int = new_slice(&mut arena, int_ty);

    let mut subst = Subster::new(&[], &[]);
    assert!(subst.is_identity());
    assert_eq!(subst.typ(&mut arena, &mut objs, int_ty), int_ty);
    assert_eq!(subst.typ(&mut arena, &mut objs, slice_int), slice_int);
}
