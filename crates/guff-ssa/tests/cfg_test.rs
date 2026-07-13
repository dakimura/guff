use guff_ssa::builder::Builder;
use guff_ssa::program::Program;
use guff_ssa::mode::BuilderMode;
use guff_ssa::{ArenaId, ids::FuncId};
use guff_ssa::value::Value;
use std::num::NonZeroU32;

#[test]
fn test_cfg_basic() {
    let mut prog = Program::new(
        BuilderMode::default(),
        guff_types::Info::default(),
        guff_types::TypeArena::new(),
        guff_types::ObjectArena::new(),
        guff_types::PackageArena::new(),
    );
    let func_id = guff_ssa::create::create_function(&mut prog, "test".to_string(), None, None);
    
    let mut builder = Builder::new(&mut prog, func_id);
    
    let entry = builder.new_basic_block("entry".to_string());
    let t_block = builder.new_basic_block("true".to_string());
    let f_block = builder.new_basic_block("false".to_string());
    let exit = builder.new_basic_block("exit".to_string());
    
    // entry -> (if) -> true, false
    builder.set_block(Some(entry));
    let cond = Value::Global(unsafe { std::mem::transmute(NonZeroU32::new(1).unwrap()) });
    builder.emit_if(cond, t_block, f_block);
    
    // true -> exit
    builder.set_block(Some(t_block));
    builder.emit_jump(exit);
    
    // false -> exit
    builder.set_block(Some(f_block));
    builder.emit_jump(exit);
    
    prog.functions.get_mut(func_id).finish_body();
    
    // Verify CFG
    assert_eq!(prog.functions.get(func_id).blocks.len(), 4);
    
    let entry_b = prog.functions.get(func_id).blocks.get(entry);
    assert_eq!(entry_b.succs.len(), 2);
    assert_eq!(entry_b.succs[0], t_block);
    assert_eq!(entry_b.succs[1], f_block);
    
    let t_b = prog.functions.get(func_id).blocks.get(t_block);
    assert_eq!(t_b.preds.len(), 1);
    assert_eq!(t_b.preds[0], entry);
    assert_eq!(t_b.succs.len(), 1);
    assert_eq!(t_b.succs[0], exit);
    
    let exit_b = prog.functions.get(func_id).blocks.get(exit);
    assert_eq!(exit_b.preds.len(), 2);
    assert!(exit_b.preds.contains(&t_block));
    assert!(exit_b.preds.contains(&f_block));
}
