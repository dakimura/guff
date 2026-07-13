//! Tail-call emission and instance build-strategy dispatch tests
//! (Milestone E, chunk E13).

use guff_ssa::create::create_function;
use guff_ssa::emit::emit_tail_call;
use guff_ssa::function::BuildStrategy;
use guff_ssa::ids::{BlockId, FuncId};
use guff_ssa::instr::{CallCommon, InstrData};
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::{Builtin, Program};
use guff_ssa::value::Value;
use guff_types::{
    basic::{init_universe, BasicKind},
    bind_tparams, new_param,
    object::type_name::new_type_name,
    signature::{new_signature_type, signature_set_type_params},
    tuple::new_tuple,
    typeparam::new_type_param,
    Info, ObjectArena, PackageArena, TypeId,
};

fn dummy_callee(prog: &mut Program, typ: TypeId) -> Value {
    let id = prog.builtins.alloc(Builtin { name: "callee".to_string(), typ });
    Value::Builtin(id)
}

/// Build a program + function `f` whose signature results are the given basic
/// kinds (no params). Returns (prog, fid, entry block, resolved result types).
fn setup_with_results(kinds: &[BasicKind]) -> (Program, FuncId, BlockId, Vec<TypeId>) {
    let (mut arena, table) = init_universe();
    let result_types: Vec<TypeId> = kinds.iter().map(|&k| table[k as usize]).collect();
    let mut objs = ObjectArena::new();
    let result_vars: Vec<_> = result_types
        .iter()
        .map(|&t| new_param(&mut objs, "", t))
        .collect();
    let results = new_tuple(&mut arena, &result_vars);
    let sig = new_signature_type(&mut arena, None, &[], &[], None, results, false);

    let mut prog = Program::new(BuilderMode::default(), Info::default(), arena, objs, PackageArena::new());
    let fid = create_function(&mut prog, "f".to_string(), None, None);
    prog.functions.get_mut(fid).signature = Some(sig);
    let block = {
        let mut b = guff_ssa::builder::Builder::new(&mut prog, fid);
        let e = b.new_basic_block("entry".to_string());
        b.set_block(Some(e));
        e
    };
    (prog, fid, block, result_types)
}

/// A single-result tail call returns the call value directly.
#[test]
fn test_emit_tail_call_single() {
    let (mut prog, fid, block, rts) = setup_with_results(&[BasicKind::Int]);
    let int_ty = rts[0];
    let callee = dummy_callee(&mut prog, int_ty);
    emit_tail_call(&mut prog, fid, block, CallCommon { value: callee, method: None, args: vec![] });

    // The block holds: a Call (typed int) followed by a Return of that call.
    let instrs: Vec<_> = prog.functions.get(fid).blocks.get(block).instrs.clone();
    let kinds: Vec<&InstrData> = instrs.iter().map(|&i| prog.functions.get(fid).instrs.get(i)).collect();
    assert!(matches!(kinds[0], InstrData::Call(c) if c.typ == int_ty));
    match kinds[1] {
        InstrData::Return(r) => {
            assert_eq!(r.results.len(), 1);
            assert_eq!(r.results[0], Value::Instr(instrs[0]));
        }
        other => panic!("expected Return, got {other:?}"),
    }
}

/// A 0-result tail call emits a void call typed as the empty tuple and a bare
/// `return`. go/ssa still numbers the call value and prints its type as "()".
#[test]
fn test_emit_tail_call_void() {
    let (mut prog, fid, block, _rts) = setup_with_results(&[]);
    // The callee's own type is irrelevant here (emit_tail_call never reads it).
    let bool_ty = prog.basic_type(BasicKind::Bool);
    let callee = dummy_callee(&mut prog, bool_ty);
    emit_tail_call(&mut prog, fid, block, CallCommon { value: callee, method: None, args: vec![] });

    // The block holds exactly: a void Call followed by a resultless Return.
    let instrs: Vec<_> = prog.functions.get(fid).blocks.get(block).instrs.clone();
    assert_eq!(instrs.len(), 2, "void tail call is Call + Return");
    match prog.functions.get(fid).instrs.get(instrs[0]) {
        InstrData::Call(c) => assert_eq!(
            guff_types::tuple_len(&prog.type_arena, Some(c.typ)),
            0,
            "call typed as the empty tuple"
        ),
        other => panic!("expected Call, got {other:?}"),
    }
    match prog.functions.get(fid).instrs.get(instrs[1]) {
        InstrData::Return(r) => assert!(r.results.is_empty(), "bare return"),
        other => panic!("expected Return, got {other:?}"),
    }

    // The disassembler renders the void call with a register and the "()" type.
    let text = disassemble_function(prog.functions.get(fid), &prog);
    assert!(text.contains("t0 = callee()"), "void call register:\n{text}");
    assert!(
        text.lines().any(|l| l.contains("callee()") && l.trim_end().ends_with("()")),
        "void call result type rendered as ():\n{text}"
    );
}

/// A multi-result tail call extracts each component into the return.
#[test]
fn test_emit_tail_call_multi() {
    let (mut prog, fid, block, rts) = setup_with_results(&[BasicKind::Int, BasicKind::String]);
    let int_ty = rts[0];
    let callee = dummy_callee(&mut prog, int_ty);
    emit_tail_call(&mut prog, fid, block, CallCommon { value: callee, method: None, args: vec![] });

    let text = disassemble_function(prog.functions.get(fid), &prog);
    assert!(text.contains("extract "), "expected extracts:\n{text}");
    // Two extracts and one return of two values.
    assert_eq!(text.matches("extract ").count(), 2, "two extracts:\n{text}");

    let instrs: Vec<_> = prog.functions.get(fid).blocks.get(block).instrs.clone();
    // Call, extract#0, extract#1, return.
    assert!(matches!(prog.functions.get(fid).instrs.get(instrs[0]), InstrData::Call(_)));
    assert!(matches!(prog.functions.get(fid).instrs.get(instrs[1]), InstrData::Extract(e) if e.index == 0));
    assert!(matches!(prog.functions.get(fid).instrs.get(instrs[2]), InstrData::Extract(e) if e.index == 1));
    match prog.functions.get(fid).instrs.get(instrs[3]) {
        InstrData::Return(r) => assert_eq!(r.results.len(), 2),
        other => panic!("expected Return, got {other:?}"),
    }
}

/// Build a generic origin `func[T any](x T) T` in the given mode; returns
/// (prog, int, origin).
fn setup_generic(mode: BuilderMode, from_syntax: bool) -> (Program, TypeId, FuncId) {
    let (mut arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let mut objs = ObjectArena::new();
    let t_obj = new_type_name(&mut objs, "T", None);
    let tparam = new_type_param(&mut arena, t_obj, None);
    let tlist = bind_tparams(&mut arena, vec![tparam]).unwrap();
    let x = new_param(&mut objs, "x", tparam);
    let params = new_tuple(&mut arena, &[x]);
    let r = new_param(&mut objs, "", tparam);
    let results = new_tuple(&mut arena, &[r]);
    let sig = new_signature_type(&mut arena, None, &[], &[], params, results, false);
    signature_set_type_params(&mut arena, sig, tlist);

    let mut prog = Program::new(mode, Info::default(), arena, objs, PackageArena::new());
    let origin = create_function(&mut prog, "F".to_string(), None, None);
    {
        let f = prog.functions.get_mut(origin);
        f.signature = Some(sig);
        f.from_syntax = from_syntax;
    }
    (prog, int_ty, origin)
}

/// `build_instance` dispatches an InstantiationWrapper instance to the wrapper
/// builder, producing a body.
#[test]
fn test_build_instance_wrapper() {
    let (mut prog, int_ty, origin) = setup_generic(BuilderMode::default(), false);
    let inst = prog.instance(origin, &[], &[int_ty]);
    assert_eq!(prog.functions.get(inst).build_strategy, BuildStrategy::InstantiationWrapper);

    prog.build_instance(inst);

    let text = disassemble_function(prog.functions.get(inst), &prog);
    assert!(text.contains("= F("), "origin call missing:\n{text}");
    assert!(text.contains("return"), "return missing:\n{text}");
}

/// `build_instance` on a ParamsOnly instance creates parameters but no body.
#[test]
fn test_build_instance_params_only() {
    let (mut prog, int_ty, origin) = setup_generic(BuilderMode::INSTANTIATE_GENERICS, false);
    let inst = prog.instance(origin, &[], &[int_ty]);
    assert_eq!(prog.functions.get(inst).build_strategy, BuildStrategy::ParamsOnly);

    prog.build_instance(inst);

    let f = prog.functions.get(inst);
    assert_eq!(f.params.len(), 1, "one parameter created");
    assert!(f.blocks.is_empty(), "no body blocks");
    assert!(f.subst.is_none(), "subst cleared");
}
