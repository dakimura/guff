//! Tests for `ssautil::switch` (Milestone F, chunk F03).

use guff::token::Token;
use guff_ssa::builder::Builder;
use guff_ssa::function::Parameter;
use guff_ssa::instr::{BinOp, Extract, InstrData, TypeAssert};
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_ssa::ssautil::switches;
use guff_ssa::value::Value;
use guff_types::{init_universe, new_interface_type, new_param, new_tuple, BasicKind, Info, ObjectArena, PackageArena};

/// Builds `if x == 1 { } else if x == 2 { } else { }` by hand and checks that
/// `switches` recovers a two-case value switch on `x`.
#[test]
fn test_value_switch_if_else_chain() {
    let (type_arena, universe) = init_universe();
    let mut prog = Program::new(
        BuilderMode::NAIVE_FORM,
        Info::default(),
        type_arena,
        ObjectArena::new(),
        PackageArena::new(),
    );
    let fid = guff_ssa::create::create_function(&mut prog, "f".into(), None, None);
    let typ_int = universe[BasicKind::Int as usize];
    let typ_bool = prog.basic_type(BasicKind::Bool);

    let param_id = prog.functions.get_mut(fid).params.alloc(Parameter {
        name: "x".into(),
        typ: typ_int,
        parent: fid,
        object: None,
    });
    let x_param = Value::Param(param_id);

    {
        let mut b = Builder::new(&mut prog, fid);
        let entry = b.new_basic_block("entry".into());
        let body1 = b.new_basic_block("case1".into());
        let b1 = b.new_basic_block("cmp2".into());
        let body2 = b.new_basic_block("case2".into());
        let default = b.new_basic_block("default".into());

        b.set_block(Some(entry));
        let one = b.prog.emit_const(Some(guff_constant::make_int64(1)), typ_int);
        let cmp1 = b.emit(InstrData::BinOp(BinOp {
            op: Token::EQL,
            x: x_param,
            y: one,
            typ: typ_bool,
        }));
        b.emit_if(Value::Instr(cmp1), body1, b1);

        b.set_block(Some(body1));
        b.emit_jump(default);

        b.set_block(Some(b1));
        let two = b.prog.emit_const(Some(guff_constant::make_int64(2)), typ_int);
        let cmp2 = b.emit(InstrData::BinOp(BinOp {
            op: Token::EQL,
            x: x_param,
            y: two,
            typ: typ_bool,
        }));
        b.emit_if(Value::Instr(cmp2), body2, default);

        b.set_block(Some(body2));
        b.emit_jump(default);

        b.set_block(Some(default));
        b.emit(InstrData::Return(guff_ssa::instr::Return { results: vec![] }));
    }

    // Switch discovery runs on the CFG before blockopt mutates the if/else chain.
    guff_ssa::dom::build_dom_tree(prog.functions.get_mut(fid));
    let f = prog.functions.get(fid);
    let sws = switches(&prog, f);
    assert_eq!(sws.len(), 1, "expected one inferred switch");
    let sw = &sws[0];
    assert_eq!(sw.const_cases.len(), 2);
    assert_eq!(sw.x, x_param);
    assert!(sw.default.is_some());
}

/// Type switch with two constant type cases on the same operand.
#[test]
fn test_type_switch_chain() {
    let (type_arena, universe) = init_universe();
    let mut prog = Program::new(
        BuilderMode::NAIVE_FORM,
        Info::default(),
        type_arena,
        ObjectArena::new(),
        PackageArena::new(),
    );
    let fid = guff_ssa::create::create_function(&mut prog, "g".into(), None, None);
    let typ_int = universe[BasicKind::Int as usize];
    let typ_string = universe[BasicKind::String as usize];
    let typ_bool = universe[BasicKind::Bool as usize];
    let empty_iface = new_interface_type(&mut prog.type_arena, vec![], vec![]);

    let param_id = prog.functions.get_mut(fid).params.alloc(Parameter {
        name: "x".into(),
        typ: empty_iface,
        parent: fid,
        object: None,
    });
    let x_param = Value::Param(param_id);

    let tuple_int_ok = {
        let v0 = new_param(&mut prog.object_arena, "value", typ_int);
        let v1 = new_param(&mut prog.object_arena, "ok", typ_bool);
        new_tuple(&mut prog.type_arena, &[v0, v1]).unwrap()
    };
    let tuple_str_ok = {
        let v0 = new_param(&mut prog.object_arena, "value", typ_string);
        let v1 = new_param(&mut prog.object_arena, "ok", typ_bool);
        new_tuple(&mut prog.type_arena, &[v0, v1]).unwrap()
    };

    {
        let mut b = Builder::new(&mut prog, fid);
        let entry = b.new_basic_block("entry".into());
        let body1 = b.new_basic_block("intcase".into());
        let b1 = b.new_basic_block("cmp2".into());
        let body2 = b.new_basic_block("strcase".into());
        let default = b.new_basic_block("default".into());

        let emit_type_case = |b: &mut Builder,
                                    block: guff_ssa::ids::BlockId,
                                    assert_ty: guff_types::TypeId,
                                    tuple_ty: guff_types::TypeId,
                                    body: guff_ssa::ids::BlockId,
                                    fallthrough: guff_ssa::ids::BlockId| {
            b.set_block(Some(block));
            let ta = b.emit(InstrData::TypeAssert(TypeAssert {
                x: x_param,
                assert_type: assert_ty,
                comma_ok: true,
                typ: tuple_ty,
            }));
            let _binding = b.emit(InstrData::Extract(Extract {
                tuple: Value::Instr(ta),
                index: 0,
                typ: assert_ty,
            }));
            let ok = b.emit(InstrData::Extract(Extract {
                tuple: Value::Instr(ta),
                index: 1,
                typ: typ_bool,
            }));
            b.emit_if(Value::Instr(ok), body, fallthrough);
        };

        emit_type_case(&mut b, entry, typ_int, tuple_int_ok, body1, b1);
        emit_type_case(&mut b, b1, typ_string, tuple_str_ok, body2, default);

        b.set_block(Some(body1));
        b.emit_jump(default);
        b.set_block(Some(body2));
        b.emit_jump(default);
        b.set_block(Some(default));
        b.emit(InstrData::Return(guff_ssa::instr::Return { results: vec![] }));
    }

    // Switch discovery runs on the CFG before blockopt mutates the if/else chain.
    guff_ssa::dom::build_dom_tree(prog.functions.get_mut(fid));
    let f = prog.functions.get(fid);
    let sws = switches(&prog, f);
    assert_eq!(sws.len(), 1, "expected one type switch");
    assert_eq!(sws[0].type_cases.len(), 2);
    assert_eq!(sws[0].x, x_param);
}
