use guff_ssa::{Program, BuilderMode, ids::{FuncId, BlockId}, instr::InstrData, blockopt, dom, ArenaId};
use guff_types::{Info, TypeArena, ObjectArena, PackageArena};

#[test]
fn test_blockopt_unreachable() {
    let mut prog = Program::new(
        BuilderMode::default(),
        Info::default(),
        TypeArena::new(),
        ObjectArena::new(),
        PackageArena::new(),
    );
    let func_id = guff_ssa::create::create_function(&mut prog, "f".to_string(), None, None);
    
    // 0 -> 1, 3
    // 2 (unreachable)
    let (b0, b1, b2, b3) = {
        let f = prog.functions.get_mut(func_id);
        let b0 = f.blocks.alloc(guff_ssa::BasicBlock::new(0, func_id));
        let b1 = f.blocks.alloc(guff_ssa::BasicBlock::new(1, func_id));
        let b2 = f.blocks.alloc(guff_ssa::BasicBlock::new(2, func_id));
        let b3 = f.blocks.alloc(guff_ssa::BasicBlock::new(3, func_id));
        
        f.blocks.get_mut(b0).succs.push(b1);
        f.blocks.get_mut(b0).succs.push(b3);
        f.blocks.get_mut(b1).preds.push(b0);
        f.blocks.get_mut(b3).preds.push(b0);
        
        (b0, b1, b2, b3)
    };
    
    blockopt::optimize_blocks(prog.functions.get_mut(func_id));
    
    let f = prog.functions.get(func_id);
    assert!(!f.blocks.get(b0).deleted);
    assert!(!f.blocks.get(b1).deleted);
    assert!(!f.blocks.get(b3).deleted);
    assert!(f.blocks.get(b2).deleted);
}

#[test]
fn test_blockopt_fuse() {
    let mut prog = Program::new(
        BuilderMode::default(),
        Info::default(),
        TypeArena::new(),
        ObjectArena::new(),
        PackageArena::new(),
    );
    let func_id = guff_ssa::create::create_function(&mut prog, "f".to_string(), None, None);
    
    // 0 -> 1
    let (b0, b1) = {
        let f = prog.functions.get_mut(func_id);
        let b0 = f.blocks.alloc(guff_ssa::BasicBlock::new(0, func_id));
        let b1 = f.blocks.alloc(guff_ssa::BasicBlock::new(1, func_id));
        
        // Add a jump at the end of b0
        let jump = f.instrs.alloc(InstrData::Jump(guff_ssa::instr::Jump {}));
        f.blocks.get_mut(b0).instrs.push(jump);
        
        f.blocks.get_mut(b0).succs.push(b1);
        f.blocks.get_mut(b1).preds.push(b0);
        
        (b0, b1)
    };
    
    blockopt::optimize_blocks(prog.functions.get_mut(func_id));
    
    let f = prog.functions.get(func_id);
    assert!(!f.blocks.get(b0).deleted);
    assert!(f.blocks.get(b1).deleted);
    // b1 succs should now be b0 succs (empty here)
    assert_eq!(f.blocks.get(b0).succs.len(), 0);
}
