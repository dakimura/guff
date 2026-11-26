use guff_ssa::{Program, BuilderMode, lift, ids::FuncId, instr::InstrData, value::Value, dom, ArenaId, builder::Builder};
use guff_types::{Info, TypeArena, ObjectArena, PackageArena, init_universe, BasicKind};
use guff::token::Token;

#[test]
fn test_lift_basic() {
    let (type_arena, universe) = init_universe();
    let mut prog = Program::new(
        BuilderMode::default(),
        Info::default(),
        type_arena,
        ObjectArena::new(),
        PackageArena::new(),
    );
    let func_id = guff_ssa::create::create_function(&mut prog, "f".to_string(), None, None);
    
    let typ_int = universe[BasicKind::Int as usize];
    let b_done_id;
    let x_alloc;

    {
        let mut b = guff_ssa::builder::Builder::new(&mut prog, func_id);
        let b0 = b.new_basic_block("entry".to_string());
        b.set_block(Some(b0));
        
        // An Alloc's value type is the pointer `*T` (Go: `Alloc.Type()`).
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
        b_done_id = b_done;
        
        // if cond (Use a dummy const)
        let cond = b.prog.emit_const(None, typ_int); // Dummy
        b.emit_if(cond, b_then, b_else);
        
        // then: x = 1
        b.set_block(Some(b_then));
        let one = b.prog.emit_const(None, typ_int); // Dummy 1
        b.emit_store(Value::Instr(x_alloc), one, guff::NO_POS);
        b.emit_jump(b_done);
        
        // else: x = 2
        b.set_block(Some(b_else));
        let two = b.prog.emit_const(None, typ_int); // Dummy 2
        b.emit_store(Value::Instr(x_alloc), two, guff::NO_POS);
        b.emit_jump(b_done);
        
        // done: print(x)
        b.set_block(Some(b_done));
        let x_val = b.emit_load(Value::Instr(x_alloc), typ_int);
        b.emit(InstrData::Return(guff_ssa::instr::Return { results: vec![x_val] }));
    }
    
    dom::build_dom_tree(prog.functions.get_mut(func_id));
    lift::lift(&mut prog, func_id);
    
    let f = prog.functions.get(func_id);
    let b_done = f.blocks.get(b_done_id);
    
    // The first instruction in b_done should be a Phi.
    let phi_id = b_done.instrs[0];
    if let InstrData::Phi(phi) = f.instrs.get(phi_id) {
        assert_eq!(phi.edges.len(), 2);
        // edges should be Some(Value)
        assert!(phi.edges[0].is_some());
        assert!(phi.edges[1].is_some());
    } else {
        panic!("expected Phi at b_done[0], got {:?}", f.instrs.get(phi_id));
    }
    
    // Entry block should NOT have Alloc if it was lifted.
    let b_entry = f.blocks.get(guff_ssa::ids::BlockId::from_index(0));
    for &id in &b_entry.instrs {
        if let InstrData::Alloc(a) = f.instrs.get(id) {
            if a.index >= 0 {
                panic!("Alloc for x should have been removed from instrs");
            }
        }
    }
}
