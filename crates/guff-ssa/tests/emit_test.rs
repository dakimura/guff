use guff_ssa::block::BasicBlock;
use guff_ssa::builder::Builder;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_types::basic::{init_universe, BasicKind};

#[test]
fn test_emit_basic() {
    // A real universe (and so a real `*int`): `emit_store` reads the address's
    // pointee type to convert the stored value, the way go/ssa's `emitStore`
    // does, so a fabricated TypeId is no longer enough here.
    let (arena, table) = init_universe();
    let int_ty = table[BasicKind::Int as usize];

    let mut prog = Program::new(
        BuilderMode::default(),
        guff_types::Info::default(),
        arena,
        guff_types::ObjectArena::new(),
        guff_types::PackageArena::new(),
    );
    let func_id = guff_ssa::create::create_function(&mut prog, "test".to_string(), None, None);

    let block_id = {
        let func = prog.functions.get_mut(func_id);
        func.blocks.alloc(BasicBlock::new(0, func_id))
    };

    // `local int` — an Alloc, whose value type is `*int`.
    let addr = guff_ssa::emit::emit_local(
        &mut prog,
        func_id,
        block_id,
        int_ty,
        guff::NO_POS,
        "x".to_string(),
    );

    let mut builder = Builder::new(&mut prog, func_id);
    builder.set_block(Some(block_id));

    let load_val = builder.emit_load(addr, int_ty);
    builder.emit_store(addr, load_val, guff::NO_POS);

    // Alloc, UnOp (load), Store — the store needs no conversion, `int` to `int`.
    let block = prog.functions.get(func_id).blocks.get(block_id);
    assert_eq!(block.instrs.len(), 3);

    let instr1 = prog.functions.get(func_id).instrs.get(block.instrs[1]);
    match instr1 {
        guff_ssa::instr::InstrData::UnOp(unop) => {
            assert_eq!(unop.x, addr);
            assert_eq!(unop.typ, int_ty);
        }
        _ => panic!("expected UnOp"),
    }

    let instr2 = prog.functions.get(func_id).instrs.get(block.instrs[2]);
    match instr2 {
        guff_ssa::instr::InstrData::Store(store) => {
            assert_eq!(store.addr, addr);
            assert_eq!(store.val, load_val);
        }
        _ => panic!("expected Store"),
    }
}
