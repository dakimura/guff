//! Generic instantiation tests (Milestone E, chunk E10).
//!
//! Exercises the instantiate.go data-model port: `create_instance` (signature
//! computation + build-strategy selection), `instance` caching, and `targstr`.
//! Only the function case is covered (method instances are deferred).

use guff_ssa::create::create_function;
use guff_ssa::function::BuildStrategy;
use guff_ssa::ids::FuncId;
use guff_ssa::instantiate::targstr;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_types::{
    basic::{init_universe, BasicKind},
    bind_tparams, new_param,
    object::type_name::new_type_name,
    signature::{new_signature_type, signature_params, signature_results, signature_set_type_params},
    tuple::new_tuple,
    typeparam::new_type_param,
    Info, ObjectArena, ObjectData, PackageArena, TypeData, TypeId,
};

/// Build a program whose type arena holds a generic origin signature
/// `func[T any](x T) T`. Returns the program, `int`, the signature, and the
/// type parameter `T`.
fn setup(mode: BuilderMode) -> (Program, TypeId, TypeId, TypeId) {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let mut objs = ObjectArena::new();

    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);
    let tlist = bind_tparams(&mut arena, vec![tparam]).expect("non-empty tparam list");

    let x = new_param(&mut objs, "x", tparam);
    let params = new_tuple(&mut arena, &[x]);
    let r = new_param(&mut objs, "", tparam);
    let results = new_tuple(&mut arena, &[r]);
    let sig = new_signature_type(&mut arena, None, &[], &[], params, results, false);
    signature_set_type_params(&mut arena, sig, tlist);

    let prog = Program::new(mode, Info::default(), arena, objs, PackageArena::new());
    (prog, int_ty, sig, tparam)
}

/// Register a generic origin function `F` with the given signature.
fn make_origin(prog: &mut Program, sig: TypeId, from_syntax: bool) -> FuncId {
    let id = create_function(prog, "F".to_string(), None, None);
    let f = prog.functions.get_mut(id);
    f.signature = Some(sig);
    f.from_syntax = from_syntax;
    id
}

/// The first element type of a tuple type.
fn tuple_first_type(prog: &Program, tuple: TypeId) -> TypeId {
    let var = match prog.type_arena.get(tuple) {
        TypeData::Tuple(t) => t.at(0),
        _ => panic!("expected tuple"),
    };
    match prog.object_arena.get(var) {
        ObjectData::Var(v) => v.typ(),
        _ => panic!("expected var"),
    }
}

/// A concrete instance under `InstantiateGenerics` with syntax builds directly
/// (FromSyntax), records its type arguments, origin, subster, and an
/// instantiated signature; `targstr` names it `F[int]`.
#[test]
fn test_instance_from_syntax() {
    let (mut prog, int_ty, sig, _) = setup(BuilderMode::INSTANTIATE_GENERICS);
    let origin = make_origin(&mut prog, sig, true);

    let inst = prog.instance(origin, &[], &[int_ty]);
    let f = prog.functions.get(inst);

    assert_eq!(f.name, "F[int]");
    assert_eq!(f.build_strategy, BuildStrategy::FromSyntax);
    assert_eq!(f.top_level_origin, Some(origin));
    assert_eq!(f.type_args, vec![int_ty]);
    assert!(f.subst.is_some(), "FromSyntax instances carry a subster");
    assert_eq!(f.synthetic.as_deref(), Some("instance of F"));

    // The instantiated signature has T replaced by int in both param and result.
    let inst_sig = f.signature.expect("instance has a signature");
    assert_ne!(inst_sig, sig, "instance signature differs from the origin");
    let params = signature_params(&prog.type_arena, inst_sig).expect("has params");
    let results = signature_results(&prog.type_arena, inst_sig).expect("has results");
    assert_eq!(tuple_first_type(&prog, params), int_ty);
    assert_eq!(tuple_first_type(&prog, results), int_ty);
}

/// Instances are cached on the origin, keyed by canonical type-argument list:
/// re-requesting the same arguments returns the identical function.
#[test]
fn test_instance_cached() {
    let (mut prog, int_ty, sig, _) = setup(BuilderMode::INSTANTIATE_GENERICS);
    let origin = make_origin(&mut prog, sig, true);

    let a = prog.instance(origin, &[], &[int_ty]);
    let b = prog.instance(origin, &[], &[int_ty]);
    assert_eq!(a, b, "same type arguments return the cached instance");
    assert_eq!(
        prog.functions.get(origin).generic_instances.len(),
        1,
        "only one instance is cached"
    );
}

/// Without `InstantiateGenerics`, every instance goes through an instantiation
/// wrapper (no subster).
#[test]
fn test_instance_wrapper_without_mode() {
    let (mut prog, int_ty, sig, _) = setup(BuilderMode::default());
    let origin = make_origin(&mut prog, sig, true);

    let inst = prog.instance(origin, &[], &[int_ty]);
    let f = prog.functions.get(inst);
    assert_eq!(f.build_strategy, BuildStrategy::InstantiationWrapper);
    assert_eq!(f.synthetic.as_deref(), Some("instantiation wrapper of F"));
    assert!(f.subst.is_none());
}

/// A concrete instance under `InstantiateGenerics` but with no syntax builds
/// params-only.
#[test]
fn test_instance_params_only() {
    let (mut prog, int_ty, sig, _) = setup(BuilderMode::INSTANTIATE_GENERICS);
    let origin = make_origin(&mut prog, sig, false); // no syntax

    let inst = prog.instance(origin, &[], &[int_ty]);
    let f = prog.functions.get(inst);
    assert_eq!(f.build_strategy, BuildStrategy::ParamsOnly);
    assert!(f.subst.is_none());
    assert_eq!(f.synthetic.as_deref(), Some("instance of F"));
}

/// A parameterized (non-concrete) type argument forces an instantiation wrapper
/// even under `InstantiateGenerics`.
#[test]
fn test_instance_parameterized_arg() {
    let (mut prog, _int, sig, tparam) = setup(BuilderMode::INSTANTIATE_GENERICS);
    let origin = make_origin(&mut prog, sig, true);

    // Instantiate with the type parameter itself — still parameterized.
    let inst = prog.instance(origin, &[], &[tparam]);
    let f = prog.functions.get(inst);
    assert_eq!(f.build_strategy, BuildStrategy::InstantiationWrapper);
    assert!(f.subst.is_none());
}

/// `targstr` formats the type-argument suffix.
#[test]
fn test_targstr() {
    let (prog, int_ty, _sig, _) = setup(BuilderMode::default());
    assert_eq!(
        targstr(&prog.type_arena, &prog.object_arena, &prog.package_arena, &[int_ty]),
        "[int]"
    );
    assert_eq!(
        targstr(&prog.type_arena, &prog.object_arena, &prog.package_arena, &[]),
        ""
    );
}
