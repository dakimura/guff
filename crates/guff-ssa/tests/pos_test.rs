//! Source-position foundation tests (Milestone D, chunk D04).
//!
//! Verifies that `emit`/builder record a `token.Pos` on instructions and that
//! it resolves back to the originating source line via the `FileSet`. Checked
//! before the lift pass runs, since lifting may delete the store.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff::ast::Decl;
use guff_types::{Checker, Config};
use guff_ssa::builder::Builder;
use guff_ssa::program::Program;
use guff_ssa::mode::BuilderMode;
use guff_ssa::instr::InstrData;

#[test]
fn test_instruction_positions() {
    // Store (from `x = x + 1`) is on line 3, Return on line 4.
    let src = "package p\nfunc f(x int) int {\n\tx = x + 1\n\treturn x\n}";
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);

    let mut prog = Program::new(
        BuilderMode::default(),
        check.info,
        check.types,
        check.objects,
        check.packages,
    );

    let type_pkg_id = check.pkg;
    let ssa_pkg_id = guff_ssa::create::create_package(&mut prog, type_pkg_id);

    let fd = file.decls.iter().find_map(|d| match d {
        Decl::FuncDecl(fd) => Some(fd),
        _ => None,
    }).expect("no FuncDecl");

    let fn_id = guff_ssa::create::create_function(&mut prog, fd.name.name.clone(), None, Some(ssa_pkg_id));

    // Register parameters from the signature (create::create_params).
    let obj_f = prog.info.defs.get(&fd.name.id).unwrap().unwrap();
    let sig_id = obj_f.typ(&prog.object_arena).unwrap();
    prog.functions.get_mut(fn_id).signature = Some(sig_id);
    guff_ssa::create::create_params(&mut prog, fn_id);

    let mut builder = Builder::new(&mut prog, fn_id);
    let entry = builder.new_basic_block("entry".to_string());
    builder.set_block(Some(entry));
    builder.stmt(&guff::ast::Stmt::BlockStmt(fd.body.clone().unwrap()));
    drop(builder);

    // Inspect positions before finish_function (lift may delete the store).
    let f = prog.functions.get(fn_id);

    let mut store_line = None;
    let mut return_line = None;
    for (id, data) in f.instrs.iter() {
        let pos = f.pos(id);
        match data {
            InstrData::Store(_) => {
                assert!(pos.is_valid(), "Store should have a valid position");
                store_line = Some(fset.position(pos).line);
            }
            InstrData::Return(_) => {
                assert!(pos.is_valid(), "Return should have a valid position");
                return_line = Some(fset.position(pos).line);
            }
            _ => {}
        }
    }

    assert_eq!(store_line, Some(3), "store from `x = x + 1` should map to line 3");
    assert_eq!(return_line, Some(4), "return should map to line 4");

    // go/ssa's `builder.binop` passes `e.OpPos` to `emitArith`, which sets it on
    // the instruction, so a BinOp carries the operator's position.
    let binop_id = f.instrs.iter().find_map(|(id, d)| match d {
        InstrData::BinOp(_) => Some(id),
        _ => None,
    }).expect("expected a binop (x + 1) instruction");
    let binop_pos = f.pos(binop_id);
    assert!(binop_pos.is_valid(), "binop should carry the operator position");
    assert_eq!(
        fset.position(binop_pos).line,
        3,
        "the `+` of `x = x + 1` is on line 3"
    );
}
