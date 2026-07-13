use guff_ssa::{Function, BasicBlock, dom, BlockId, FuncId, ArenaId};

#[test]
fn test_dom_diamond() {
    let mut f = Function::new("f".to_string(), None, None);
    let func_id = FuncId::from_index(0); // Dummy
    
    // 0 -> 1, 2
    // 1 -> 3
    // 2 -> 3
    let b0 = f.blocks.alloc(BasicBlock::new(0, func_id));
    let b1 = f.blocks.alloc(BasicBlock::new(1, func_id));
    let b2 = f.blocks.alloc(BasicBlock::new(2, func_id));
    let b3 = f.blocks.alloc(BasicBlock::new(3, func_id));
    
    {
        let blocks = &mut f.blocks;
        blocks.get_mut(b0).succs = vec![b1, b2];
        blocks.get_mut(b1).preds = vec![b0];
        blocks.get_mut(b1).succs = vec![b3];
        blocks.get_mut(b2).preds = vec![b0];
        blocks.get_mut(b2).succs = vec![b3];
        blocks.get_mut(b3).preds = vec![b1, b2];
    }
    
    dom::build_dom_tree(&mut f);
    
    let blocks = &f.blocks;
    assert!(blocks.get(b0).dominates(blocks.get(b0)));
    assert!(blocks.get(b0).dominates(blocks.get(b1)));
    assert!(blocks.get(b0).dominates(blocks.get(b2)));
    assert!(blocks.get(b0).dominates(blocks.get(b3)));
    
    assert!(!blocks.get(b1).dominates(blocks.get(b3)));
    assert!(!blocks.get(b2).dominates(blocks.get(b3)));
    
    assert_eq!(blocks.get(b1).idom(), Some(b0));
    assert_eq!(blocks.get(b2).idom(), Some(b0));
    assert_eq!(blocks.get(b3).idom(), Some(b0));
}

#[test]
fn test_dom_loop() {
    let mut f = Function::new("f".to_string(), None, None);
    let func_id = FuncId::from_index(0);
    
    // 0 -> 1
    // 1 -> 2, 4
    // 2 -> 3
    // 3 -> 1
    let b0 = f.blocks.alloc(BasicBlock::new(0, func_id));
    let b1 = f.blocks.alloc(BasicBlock::new(1, func_id));
    let b2 = f.blocks.alloc(BasicBlock::new(2, func_id));
    let b3 = f.blocks.alloc(BasicBlock::new(3, func_id));
    let b4 = f.blocks.alloc(BasicBlock::new(4, func_id));
    
    {
        let blocks = &mut f.blocks;
        blocks.get_mut(b0).succs = vec![b1];
        blocks.get_mut(b1).preds = vec![b0, b3];
        blocks.get_mut(b1).succs = vec![b2, b4];
        blocks.get_mut(b2).preds = vec![b1];
        blocks.get_mut(b2).succs = vec![b3];
        blocks.get_mut(b3).preds = vec![b2];
        blocks.get_mut(b3).succs = vec![b1];
        blocks.get_mut(b4).preds = vec![b1];
    }
    
    dom::build_dom_tree(&mut f);
    
    let blocks = &f.blocks;
    assert_eq!(blocks.get(b1).idom(), Some(b0));
    assert_eq!(blocks.get(b2).idom(), Some(b1));
    assert_eq!(blocks.get(b3).idom(), Some(b2));
    assert_eq!(blocks.get(b4).idom(), Some(b1));
    
    assert!(blocks.get(b1).dominates(blocks.get(b3)));
    assert!(!blocks.get(b3).dominates(blocks.get(b1)));
}
