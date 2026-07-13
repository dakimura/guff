//! Forward goto SSA tests.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_package;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::ids::FuncId;
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_types::{Checker, Config};

fn build(src: &str, fname: &str) -> (Program, FuncId, String) {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", src.as_bytes(), Mode::NONE).expect("parse");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    let type_pkg_id = check.pkg;

    let mut prog = Program::new(
        BuilderMode::SANITY_CHECK_FUNCTIONS,
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    let ssa_pkg_id = create_package(&mut prog, type_pkg_id);
    populate_package_members(&mut prog, ssa_pkg_id, &[file.clone()]);
    build_package(&mut prog, ssa_pkg_id, &[file]);

    let fid: FuncId = match prog.packages.get(ssa_pkg_id).members.get(fname) {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected {fname}, got {other:?}"),
    };
    let asm = disassemble_function(prog.functions.get(fid), &prog);
    (prog, fid, asm)
}

#[test]
fn forward_goto_jumps_to_label() {
    const SRC: &str = "\
package p

func f() {
	goto L
	_ = 1
L:
	return
}
";
    let (_prog, fid, asm) = build(SRC, "f");
    let nblocks = _prog.functions.get(fid).blocks.len();
    assert!(nblocks >= 2, "expected multiple blocks for goto, got {nblocks}:\n{asm}");
}

#[test]
fn forward_goto_before_assignment() {
    const SRC: &str = "\
package p

func f() int {
	goto L
	x := 1
L:
	return x
}
";
    let (_prog, fid, asm) = build(SRC, "f");
    let nblocks = _prog.functions.get(fid).blocks.len();
    assert!(nblocks >= 2, "expected multiple blocks, got {nblocks}:\n{asm}");
}
