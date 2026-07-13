use guff_ssa::builder::Builder;
use guff_ssa::program::Program;
use guff_ssa::mode::BuilderMode;
use guff_ssa::{ArenaId, ids::BlockId};
use guff_ssa::value::Value;
use guff::ast::{Expr, Ident, AssignStmt};
use guff::token::Token;
use guff_types::{TypeId, TypeAndValue, Info, OperandMode};
use guff_constant::Value as ConstantValue;
use std::num::NonZeroU32;

#[test]
fn test_stmt_assign() {
    let mut prog = Program::new(
        BuilderMode::default(),
        Info::default(),
        guff_types::TypeArena::new(),
        guff_types::ObjectArena::new(),
        guff_types::PackageArena::new(),
    );
    
    let typ = unsafe { std::mem::transmute::<NonZeroU32, TypeId>(NonZeroU32::new(1).unwrap()) };
    
    // Setup local variable 'x'
    let x_id = 1;
    let x_ident = Ident { id: x_id, name: "x".to_string(), ..Default::default() };
    let obj_x = guff_types::object::var::new_var(&mut prog.object_arena, "x", typ);
    prog.info.uses.insert(x_id, obj_x);
    prog.info.types.insert(x_id, TypeAndValue {
        mode: OperandMode::Variable,
        typ,
        val: None,
    });
    
    let func_id = guff_ssa::create::create_function(&mut prog, "test".to_string(), None, None);
    
    // Manually add 'x' as an Alloc to the function objects
    let (entry, alloc_x) = {
        let mut builder = Builder::new(&mut prog, func_id);
        let entry = builder.new_basic_block("entry".to_string());
        builder.set_block(Some(entry));
        let ptr = guff_types::new_pointer(&mut builder.prog.type_arena, typ);
        let id = builder.emit(guff_ssa::instr::InstrData::Alloc(guff_ssa::instr::Alloc {
            typ: ptr,
            heap: false,
            comment: "x".to_string(),
            index: -1,
        }));
        let v = Value::Instr(id);
        builder.func_mut().objects.insert(obj_x, v);
        builder.func_mut().locals.push(id);
        (entry, id)
    };

    // x = 42
    let lit_id = 2;
    let lit = guff::ast::BasicLit { id: lit_id, value: "42".to_string(), kind: Some(Token::INT), ..Default::default() };
    prog.info.types.insert(lit_id, TypeAndValue {
        mode: OperandMode::Constant,
        typ,
        val: Some(ConstantValue::Int64(42)),
    });

    let mut builder = Builder::new(&mut prog, func_id);
    builder.set_block(Some(entry));
    
    let assign = AssignStmt {
        lhs: vec![Expr::Ident(x_ident)],
        rhs: vec![Expr::BasicLit(lit)],
        tok: Some(Token::ASSIGN),
        ..Default::default()
    };
    
    builder.stmt(&guff::ast::Stmt::AssignStmt(assign));
    
    // Verify block instructions: [Alloc, Store]
    let block = prog.functions.get(func_id).blocks.get(entry);
    assert_eq!(block.instrs.len(), 2);
    
    let instr1 = prog.functions.get(func_id).instrs.get(block.instrs[1]);
    match instr1 {
        guff_ssa::instr::InstrData::Store(store) => {
            assert_eq!(store.addr, Value::Instr(alloc_x));
            // store.val should be a constant
            match store.val {
                Value::Const(c_id) => {
                    let c = prog.constants.get(c_id);
                    assert!(guff_constant::compare(c.val.as_ref().unwrap().clone(), Token::EQL, ConstantValue::Int64(42)));
                }
                _ => panic!("expected constant value in store"),
            }
        }
        _ => panic!("expected Store"),
    }
}
