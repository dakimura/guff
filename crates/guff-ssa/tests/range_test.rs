//! Range and labelled-break SSA tests.

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
    let file = parse_file(&fset, "p.go", src.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    let type_pkg_id = check.pkg;

    let mut prog = Program::new(
        BuilderMode::default(),
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
        other => panic!("expected {fname} to be a Function member, got {other:?}"),
    };
    disassemble_function(prog.functions.get(fid), &prog)
}

#[test]
fn test_range_over_slice() {
    const SRC: &str = "\
package p

func sum(s []int) int {
	n := 0
	for _, v := range s {
		n += v
	}
	return n
}
";
    let asm = build(SRC, "sum");
    assert!(
        asm.contains("len(s)"),
        "expected len call in slice range loop:\n{asm}"
    );
    assert!(
        asm.contains("rangeindex"),
        "expected indexed range loop:\n{asm}"
    );
}

#[test]
fn test_range_over_array() {
    const SRC: &str = "\
package p

func count(a [2]int) {
	for range a {
	}
}
";
    let asm = build(SRC, "count");
    assert!(
        !asm.contains("len(a)"),
        "static array range should not call len:\n{asm}"
    );
    assert!(
        asm.contains("rangeindex"),
        "expected indexed range loop:\n{asm}"
    );
}

#[test]
fn test_range_over_channel() {
    const SRC: &str = "\
package p

func endless(ch <-chan int) {
	for range ch {
	}
}
";
    let endless_asm = build(SRC, "endless");
    assert!(
        endless_asm.contains("<-ch,ok"),
        "expected comma-ok receive in range loop:\n{endless_asm}"
    );
}

#[test]
fn test_range_over_map() {
    const SRC: &str = "\
package p

func walk(m map[string]int) {
	for range m {
	}
}
";
    let asm = build(SRC, "walk");
    assert!(asm.contains("= range "), "expected range instruction:\n{asm}");
    assert!(asm.contains("= next "), "expected next instruction:\n{asm}");
    assert!(asm.contains("rangeiter"), "expected rangeiter blocks:\n{asm}");
}

#[test]
fn test_range_over_string() {
    const SRC: &str = "\
package p

func walk(s string) {
	for _, r := range s {
		_ = r
	}
}
";
    let asm = build(SRC, "walk");
    assert!(asm.contains("= range "), "expected range instruction:\n{asm}");
    assert!(asm.contains("= next "), "expected next instruction:\n{asm}");
}

#[test]
fn test_range_over_int() {
    const SRC: &str = "\
package p

func sum(n int) int {
	total := 0
	for i := range n {
		total += i
	}
	return total
}
";
    let asm = build(SRC, "sum");
    assert!(
        asm.contains("rangeint"),
        "expected integer range loop:\n{asm}"
    );
}
