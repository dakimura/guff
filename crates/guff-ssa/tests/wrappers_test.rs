//! Instantiation-wrapper body tests (Milestone E, chunk E12).
//!
//! Builds an instantiation wrapper for a generic function instance and checks
//! its disassembly: arguments coerced to the origin's type-parameter form, a
//! call to the generic origin, and results coerced back.

use guff_ssa::create::create_function;
use guff_ssa::function::BuildStrategy;
use guff_ssa::ids::FuncId;
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_ssa::wrappers::build_instantiation_wrapper;
use guff_types::{
    basic::{init_universe, BasicKind},
    bind_tparams, new_param,
    object::type_name::new_type_name,
    signature::{new_signature_type, signature_set_type_params},
    tuple::new_tuple,
    typeparam::new_type_param,
    Info, ObjectArena, PackageArena, TypeId,
};

/// Build a program holding a generic origin `func[T any](x T) <results>` where
/// `results` is built from `result_types` (each a `T`). Returns the program,
/// `int`, and the origin function id.
fn setup(result_count: usize) -> (Program, TypeId, FuncId) {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let mut objs = ObjectArena::new();

    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);
    let tlist = bind_tparams(&mut arena, vec![tparam]).expect("non-empty tparam list");

    let x = new_param(&mut objs, "x", tparam);
    let params = new_tuple(&mut arena, &[x]);
    let result_vars: Vec<_> = (0..result_count)
        .map(|_| new_param(&mut objs, "", tparam))
        .collect();
    let results = new_tuple(&mut arena, &result_vars);
    let sig = new_signature_type(&mut arena, None, &[], &[], params, results, false);
    signature_set_type_params(&mut arena, sig, tlist);

    // Default mode (no InstantiateGenerics) so instances become wrappers.
    let mut prog = Program::new(BuilderMode::default(), Info::default(), arena, objs, PackageArena::new());
    let origin = create_function(&mut prog, "F".to_string(), None, None);
    prog.functions.get_mut(origin).signature = Some(sig);
    (prog, int_ty, origin)
}

/// A single-result wrapper: coerce the arg to `T`, call `F`, coerce the result
/// back to `int`.
#[test]
fn test_instantiation_wrapper_single_result() {
    let (mut prog, int_ty, origin) = setup(1);
    let inst = prog.instance(origin, &[], &[int_ty]);
    assert_eq!(
        prog.functions.get(inst).build_strategy,
        BuildStrategy::InstantiationWrapper
    );

    build_instantiation_wrapper(&mut prog, inst);

    let text = disassemble_function(prog.functions.get(inst), &prog);
    // arg coercion int -> T, call to origin F, result coercion T -> int, return.
    assert!(text.contains("changetype T <- int (x)"), "arg coercion missing:\n{text}");
    assert!(text.contains("= F("), "origin call missing:\n{text}");
    assert!(text.contains("changetype int <- T"), "result coercion missing:\n{text}");
    assert!(text.contains("return"), "return missing:\n{text}");
}

/// A multi-result wrapper extracts each component before coercing it back.
#[test]
fn test_instantiation_wrapper_multi_result() {
    let (mut prog, int_ty, origin) = setup(2);
    let inst = prog.instance(origin, &[], &[int_ty]);
    build_instantiation_wrapper(&mut prog, inst);

    let text = disassemble_function(prog.functions.get(inst), &prog);
    assert!(text.contains("changetype T <- int (x)"), "arg coercion missing:\n{text}");
    assert!(text.contains("= F("), "origin call missing:\n{text}");
    assert!(text.contains("extract "), "extract missing:\n{text}");
    // Two result coercions back to int.
    let n_back = text.matches("changetype int <- T").count();
    assert_eq!(n_back, 2, "expected two result coercions:\n{text}");
}
