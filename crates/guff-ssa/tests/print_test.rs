//! Disassembly fidelity tests (Milestone D, chunk D01).
//!
//! Builds a diamond CFG by hand, runs the full post-construction pipeline
//! (blockopt + dom + lift), and checks that the disassembler emits go/ssa-style
//! output: block headers with `P:`/`S:`/`idom:`, `if cond goto T else F`,
//! `jump N`, and `phi [pred: val, ...]`.

use guff_ssa::{Program, BuilderMode, ids::FuncId, instr::InstrData, value::Value};
use guff_ssa::print::disassemble_function;
use guff_types::{Info, ObjectArena, PackageArena, init_universe, BasicKind};

#[test]
fn test_disassemble_diamond() {
    let (type_arena, universe) = init_universe();
    let mut prog = Program::new(
        BuilderMode::default(),
        Info::default(),
        type_arena,
        ObjectArena::new(),
        PackageArena::new(),
    );
    let func_id: FuncId = guff_ssa::create::create_function(&mut prog, "f".to_string(), None, None);

    let typ_int = universe[BasicKind::Int as usize];
    let x_alloc;

    {
        let mut b = guff_ssa::builder::Builder::new(&mut prog, func_id);
        let b0 = b.new_basic_block("entry".to_string());
        b.set_block(Some(b0));

        let ptr_int = guff_types::new_pointer(&mut b.prog.type_arena, typ_int);
        x_alloc = b.emit(InstrData::Alloc(guff_ssa::instr::Alloc {
            typ: ptr_int,
            heap: false,
            comment: "x".to_string(),
            index: -1,
        }));

        let b_then = b.new_basic_block("then".to_string());
        let b_else = b.new_basic_block("else".to_string());
        let b_done = b.new_basic_block("done".to_string());

        let cond = b.prog.emit_const(None, typ_int);
        b.emit_if(cond, b_then, b_else);

        b.set_block(Some(b_then));
        let one = b.prog.emit_const(None, typ_int);
        b.emit_store(Value::Instr(x_alloc), one, guff::NO_POS);
        b.emit_jump(b_done);

        b.set_block(Some(b_else));
        let two = b.prog.emit_const(None, typ_int);
        b.emit_store(Value::Instr(x_alloc), two, guff::NO_POS);
        b.emit_jump(b_done);

        b.set_block(Some(b_done));
        let x_val = b.emit_load(Value::Instr(x_alloc), typ_int);
        b.emit(InstrData::Return(guff_ssa::instr::Return { results: vec![x_val] }));
    }

    prog.finish_function(func_id);

    let f = prog.functions.get(func_id);
    let out = disassemble_function(f, &prog);
    println!("{}", out);

    // Function header.
    assert!(out.contains("func f():"), "missing function header:\n{out}");

    // Block headers carry predecessor/successor counts and an immediate
    // dominator once the dom tree is built.
    assert!(out.contains("P:0 S:2"), "entry header wrong:\n{out}");
    assert!(out.contains("idom:0"), "missing idom annotation:\n{out}");

    // Control-flow terminators use go/ssa's target-numbered forms.
    assert!(out.contains("goto "), "if should print goto targets:\n{out}");
    assert!(out.lines().any(|l| l.trim().starts_with("if ")), "missing if:\n{out}");
    assert!(out.lines().any(|l| l.trim().starts_with("jump ")), "missing jump:\n{out}");

    // Lifting inserts a phi at the merge block, printed with predecessor edges.
    // Register numbering (D02) restarts at t0 per function in block/instr order,
    // so the sole surviving value instruction (the phi) is t0 regardless of its
    // arena id.
    assert!(out.contains("t0 = phi ["), "phi should be numbered t0:\n{out}");
    assert!(out.contains("return t0"), "return should reference t0:\n{out}");
}
