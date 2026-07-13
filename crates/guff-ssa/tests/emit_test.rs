use guff_ssa::builder::Builder;
use guff_ssa::function::Function;
use guff_ssa::program::Program;
use guff_ssa::mode::BuilderMode;
use guff_ssa::{ArenaId, ids::FuncId};
use guff_ssa::block::BasicBlock;
use guff_ssa::value::Value;
use guff_types::TypeId;
use std::num::NonZeroU32;

#[test]
fn test_emit_basic() {
    let mut prog = Program::new(
        BuilderMode::default(),
        guff_types::Info::default(),
        guff_types::TypeArena::new(),
        guff_types::ObjectArena::new(),
        guff_types::PackageArena::new(),
    );
    let func_id = guff_ssa::create::create_function(&mut prog, "test".to_string(), None, None);
    
    let block_id = {
        let func = prog.functions.get_mut(func_id);
        func.blocks.alloc(BasicBlock::new(0, func_id))
    };
    
    let mut builder = Builder::new(&mut prog, func_id);
    builder.set_block(Some(block_id));
    
    // Create a dummy TypeId
    let typ = unsafe { std::mem::transmute::<NonZeroU32, TypeId>(NonZeroU32::new(1).unwrap()) };
    
    // Emit a load from a global (dummy)
    let global_val = Value::Global(unsafe { std::mem::transmute(NonZeroU32::new(1).unwrap()) });
    let load_val = builder.emit_load(global_val, typ);
    
    // Emit a store
    builder.emit_store(global_val, load_val, guff::NO_POS);
    
    // Verify block contents
    let block = prog.functions.get(func_id).blocks.get(block_id);
    assert_eq!(block.instrs.len(), 2);
    
    // Verify instructions
    let instr0 = prog.functions.get(func_id).instrs.get(block.instrs[0]);
    match instr0 {
        guff_ssa::instr::InstrData::UnOp(unop) => {
            assert_eq!(unop.x, global_val);
            assert_eq!(unop.typ, typ);
        }
        _ => panic!("expected UnOp"),
    }
    
    let instr1 = prog.functions.get(func_id).instrs.get(block.instrs[1]);
    match instr1 {
        guff_ssa::instr::InstrData::Store(store) => {
            assert_eq!(store.addr, global_val);
            assert_eq!(store.val, load_val);
        }
        _ => panic!("expected Store"),
    }
}
