//! Value-coercion emit primitive tests (Milestone E, chunk E11).
//!
//! Exercises `emit_extract`, `emit_type_coercion`, and `emit_call` from
//! emit.rs — the primitives an instantiation wrapper body uses to reconcile a
//! concrete generic instance with its type-parameter origin.

use guff_ssa::create::create_function;
use guff_ssa::emit::{emit_call, emit_extract, emit_type_coercion};
use guff_ssa::function::Parameter;
use guff_ssa::ids::{BlockId, FuncId};
use guff_ssa::instr::{CallCommon, InstrData};
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_instr;
use guff_ssa::program::{Builtin, Program};
use guff_ssa::value::Value;
use guff_types::{
    basic::{init_universe, BasicKind},
    new_param,
    tuple::new_tuple,
    Info, ObjectArena, PackageArena, TypeId,
};

/// Build a program with a single function `f` and an entry block. Returns the
/// program, the function id, the entry block, and the `int`/`string` types.
fn setup() -> (Program, FuncId, BlockId, TypeId, TypeId) {
    let (arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
    let string_ty = table[BasicKind::String as usize];

    let mut prog = Program::new(
        BuilderMode::default(),
        Info::default(),
        arena,
        ObjectArena::new(),
        PackageArena::new(),
    );
    let fid = create_function(&mut prog, "f".to_string(), None, None);
    let block = {
        let mut b = guff_ssa::builder::Builder::new(&mut prog, fid);
        let e = b.new_basic_block("entry".to_string());
        b.set_block(Some(e));
        e
    };
    (prog, fid, block, int_ty, string_ty)
}

/// A `Value::Builtin` to use as a call target (its type is irrelevant here).
fn dummy_callee(prog: &mut Program, typ: TypeId) -> Value {
    let id = prog.builtins.alloc(Builtin {
        name: "callee".to_string(),
        typ,
    });
    Value::Builtin(id)
}

/// `emit_extract` produces an Extract whose type is the indexed tuple element.
#[test]
fn test_emit_extract() {
    let (mut prog, fid, block, int_ty, string_ty) = setup();

    // A tuple type (int, string).
    let v0 = new_param(&mut prog.object_arena, "", int_ty);
    let v1 = new_param(&mut prog.object_arena, "", string_ty);
    let tuple_ty = new_tuple(&mut prog.type_arena, &[v0, v1]).expect("non-empty tuple");

    // A call producing that tuple.
    let callee = dummy_callee(&mut prog, int_ty);
    let tuple_val = emit_call(
        &mut prog,
        fid,
        block,
        CallCommon { value: callee, method: None, args: vec![], ellipsis: false },
        tuple_ty,
    );

    let e0 = emit_extract(&mut prog, fid, block, tuple_val, 0);
    let e1 = emit_extract(&mut prog, fid, block, tuple_val, 1);

    // Extracted values have the corresponding element types.
    assert_eq!(result_type(&prog, fid, e0), int_ty);
    assert_eq!(result_type(&prog, fid, e1), string_ty);

    // Structure and disassembly of the first extract.
    let e0_id = match e0 {
        Value::Instr(id) => id,
        _ => panic!("extract is an instruction"),
    };
    assert!(matches!(prog.functions.get(fid).instrs.get(e0_id), InstrData::Extract(e) if e.index == 0));
    let text = disassemble_instr(e0_id, block, prog.functions.get(fid), &prog);
    assert!(text.contains("extract "), "got: {text}");
    assert!(text.contains("#0"), "got: {text}");
}

/// `emit_type_coercion` returns the value unchanged when the type matches, and
/// emits a ChangeType otherwise.
#[test]
fn test_emit_type_coercion() {
    let (mut prog, fid, block, int_ty, string_ty) = setup();

    // A parameter of type int.
    let p = {
        let f = prog.functions.get_mut(fid);
        let pid = f.params.alloc(Parameter {
            name: "x".to_string(),
            typ: int_ty,
            parent: fid,
            object: None,
        });
        Value::Param(pid)
    };

    let before = prog.functions.get(fid).instrs.len();

    // Coercing int -> int is a no-op: same value, no instruction emitted.
    let same = emit_type_coercion(&mut prog, fid, block, p, int_ty);
    assert_eq!(same, p);
    assert_eq!(prog.functions.get(fid).instrs.len(), before, "no instruction for identity coercion");

    // Coercing int -> string emits a ChangeType with the target type.
    let coerced = emit_type_coercion(&mut prog, fid, block, p, string_ty);
    assert_ne!(coerced, p);
    assert_eq!(result_type(&prog, fid, coerced), string_ty);

    let cid = match coerced {
        Value::Instr(id) => id,
        _ => panic!("coercion is an instruction"),
    };
    assert!(matches!(prog.functions.get(fid).instrs.get(cid), InstrData::ChangeType(_)));
    let text = disassemble_instr(cid, block, prog.functions.get(fid), &prog);
    assert!(text.contains("changetype string <- int"), "got: {text}");
}

/// `emit_call` produces a Call with the given result type.
#[test]
fn test_emit_call() {
    let (mut prog, fid, block, int_ty, _string) = setup();
    let callee = dummy_callee(&mut prog, int_ty);
    let v = emit_call(
        &mut prog,
        fid,
        block,
        CallCommon { value: callee, method: None, args: vec![], ellipsis: false },
        int_ty,
    );
    assert_eq!(result_type(&prog, fid, v), int_ty);
    let id = match v {
        Value::Instr(id) => id,
        _ => panic!("call is an instruction"),
    };
    assert!(matches!(prog.functions.get(fid).instrs.get(id), InstrData::Call(_)));
}

/// The result type of a value-producing instruction reference.
fn result_type(prog: &Program, fid: FuncId, v: Value) -> TypeId {
    match v {
        Value::Instr(id) => prog
            .functions
            .get(fid)
            .instrs
            .get(id)
            .result_type()
            .expect("value instruction has a result type"),
        _ => panic!("expected an instruction value"),
    }
}
