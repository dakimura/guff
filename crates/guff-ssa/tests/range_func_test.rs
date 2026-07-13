//! Range-over-func SSA tests.

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

fn build(src: &str, fname: &str) -> String {
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
    disassemble_function(prog.functions.get(fid), &prog)
}

#[test]
fn range_over_func_iterator() {
    const SRC: &str = "\
package p

func Seq(yield func(int) bool) {}

func use() {
	sum := 0
	for k := range Seq {
		sum += k
	}
	_ = sum
}
";
    let asm = build(SRC, "use");
    assert!(
        asm.contains("make closure"),
        "expected MakeClosure for yield function:\n{asm}"
    );
    assert!(
        asm.contains("rangefunc"),
        "expected rangefunc blocks:\n{asm}"
    );
}

#[test]
fn range_over_func_break() {
    const SRC: &str = "\
package p

func Seq(yield func(int) bool) {}

func use() {
	for k := range Seq {
		if k == 3 {
			break
		}
	}
}
";
    let asm = build(SRC, "use");
    assert!(
        asm.contains("rangefunc"),
        "expected rangefunc lowering:\n{asm}"
    );
}
