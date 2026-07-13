use guff_ssa::builder::Builder;
use guff_ssa::program::Program;
use guff_ssa::mode::BuilderMode;
use guff_ssa::ids::FuncId;
use guff_ssa::value::Value;
use guff::ast::{Expr, BasicLit};
use guff::token::Token;
use guff_types::{TypeId, TypeAndValue, Info, OperandMode};
use guff_constant::Value as ConstantValue;
use std::num::NonZeroU32;

#[test]
fn test_expr_basic_lit() {
    let mut prog = Program::new(
        BuilderMode::default(),
        Info::default(),
        guff_types::TypeArena::new(),
        guff_types::ObjectArena::new(),
        guff_types::PackageArena::new(),
    );
    
    let typ = unsafe { std::mem::transmute::<NonZeroU32, TypeId>(NonZeroU32::new(1).unwrap()) };
    let lit = BasicLit {
        id: 1,
        kind: Some(Token::INT),
        value: "42".to_string(),
        ..Default::default()
    };
    
    prog.info.types.insert(1, TypeAndValue {
        mode: OperandMode::Constant,
        typ,
        val: Some(ConstantValue::Int64(42)),
    });
    
    let func_id = guff_ssa::create::create_function(&mut prog, "test".to_string(), None, None);
    let mut builder = Builder::new(&mut prog, func_id);
    
    let v = builder.expr(&Expr::BasicLit(lit));
    
    match v {
        Value::Const(id) => {
            let c = prog.constants.get(id);
            assert_eq!(c.typ, typ);
            assert!(guff_constant::compare(
                c.val.as_ref().unwrap().clone(),
                Token::EQL,
                ConstantValue::Int64(42)
            ));
        }
        _ => panic!("expected constant"),
    }
}
