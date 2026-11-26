//! Alloc / local emission primitives (Milestone E-adjacent builder core,
//! chunk E15): emit_alloc / emit_local / emit_local_var and the faithful
//! `*T` value type of an Alloc.

use guff_ssa::create::create_function;
use guff_ssa::emit::{emit_alloc, emit_local, emit_local_var};
use guff_ssa::ids::{BlockId, FuncId};
use guff_ssa::instr::InstrData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_ssa::value::Value;
use guff_types::{
    basic::{init_universe, BasicKind},
    new_param, pointer_elem, Info, ObjectArena, PackageArena, TypeId,
};
use guff::NO_POS;

/// A program with one function `f` and an entry block. Returns
/// (prog, fid, entry, int type).
fn setup() -> (Program, FuncId, BlockId, TypeId) {
    let (arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];
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
    (prog, fid, block, int_ty)
}

/// emit_alloc yields an Alloc whose *value* type is the pointer `*T`, while the
/// disassembly body derefs to `local T (comment)` with `*T` right-aligned.
#[test]
fn test_emit_alloc_pointer_type_and_render() {
    let (mut prog, fid, block, int_ty) = setup();
    let v = emit_alloc(&mut prog, fid, block, int_ty, NO_POS, "x".to_string());

    let id = match v {
        Value::Instr(id) => id,
        other => panic!("expected instr value, got {other:?}"),
    };
    let f = prog.functions.get(fid);
    match f.instrs.get(id) {
        InstrData::Alloc(a) => {
            assert!(!a.heap, "stack local");
            assert_eq!(a.comment, "x");
            assert_eq!(a.index, -1);
            // The Alloc's own value type (`a.typ`) is `*int`.
            assert_eq!(
                pointer_elem(&prog.type_arena, a.typ),
                int_ty,
                "alloc value type is *int"
            );
        }
        other => panic!("expected Alloc, got {other:?}"),
    }

    let text = disassemble_function(prog.functions.get(fid), &prog);
    // Body derefs to the pointee; right-aligned column shows the `*int` value type.
    assert!(text.contains("t0 = local int (x)"), "alloc body:\n{text}");
    assert!(
        text.lines().any(|l| l.contains("local int (x)") && l.trim_end().ends_with("*int")),
        "alloc right-aligned value type is *int:\n{text}"
    );
}

/// emit_local additionally records the Alloc in the function's `locals` list.
#[test]
fn test_emit_local_records_local() {
    let (mut prog, fid, block, int_ty) = setup();
    assert_eq!(prog.functions.get(fid).locals.len(), 0);
    let v = emit_local(&mut prog, fid, block, int_ty, NO_POS, "y".to_string());
    let locals = &prog.functions.get(fid).locals;
    assert_eq!(locals.len(), 1, "one local recorded");
    assert_eq!(Value::Instr(locals[0]), v, "locals holds the alloc");
}

/// emit_local_var binds a type-checker variable to its stack cell in `objects`,
/// taking the local's type and comment from the object.
#[test]
fn test_emit_local_var_binds_object() {
    let (mut prog, fid, block, int_ty) = setup();
    let obj = new_param(&mut prog.object_arena, "z", int_ty);

    let v = emit_local_var(&mut prog, fid, block, obj);

    let f = prog.functions.get(fid);
    assert_eq!(f.objects.get(&obj), Some(&v), "object bound to its cell");
    assert_eq!(f.locals.len(), 1, "recorded as a local");
    // Comment (disassembly label) comes from the variable's name.
    let id = match v {
        Value::Instr(id) => id,
        other => panic!("expected instr, got {other:?}"),
    };
    match f.instrs.get(id) {
        InstrData::Alloc(a) => assert_eq!(a.comment, "z"),
        other => panic!("expected Alloc, got {other:?}"),
    }
}
