//! Switch / type-switch / select / send / IncDec SSA tests (R17 statement coverage).

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
fn test_value_switch() {
    const SRC: &str = "\
package p

func classify(n int) int {
	switch n {
	case 1:
		return 10
	case 2, 3:
		return 20
	default:
		return 0
	}
}
";
    let asm = build(SRC, "classify");
    assert!(
        asm.contains("switch.body") || asm.contains("if "),
        "expected switch if-else chain:\n{asm}"
    );
    assert!(
        asm.contains("=="),
        "expected equality compares for cases:\n{asm}"
    );
}

#[test]
fn test_bool_switch_and_fallthrough() {
    const SRC: &str = "\
package p

func flags(a, b bool) int {
	n := 0
	switch {
	case a:
		n = 1
		fallthrough
	case b:
		n = n + 2
	}
	return n
}
";
    let asm = build(SRC, "flags");
    assert!(
        asm.contains("jump ") || asm.contains("if "),
        "expected bool-switch CFG:\n{asm}"
    );
}

#[test]
fn test_type_switch() {
    const SRC: &str = "\
package p

func describe(x interface{}) int {
	switch v := x.(type) {
	case int:
		return v
	case string:
		return len(v)
	default:
		return -1
	}
}
";
    let asm = build(SRC, "describe");
    assert!(
        asm.contains("typeassert"),
        "expected typeassert in type switch:\n{asm}"
    );
}

#[test]
fn test_send_and_incdec() {
    const SRC: &str = "\
package p

func pump(ch chan int) {
	i := 0
	i++
	ch <- i
	i--
}
";
    let asm = build(SRC, "pump");
    assert!(asm.contains("send "), "expected send instruction:\n{asm}");
    assert!(
        asm.contains("+") || asm.contains("-"),
        "expected IncDec arithmetic:\n{asm}"
    );
}

#[test]
fn test_select_blocking() {
    const SRC: &str = "\
package p

func race(a, b <-chan int) int {
	select {
	case x := <-a:
		return x
	case y := <-b:
		return y
	}
}
";
    let asm = build(SRC, "race");
    assert!(
        asm.contains("select blocking"),
        "expected blocking select:\n{asm}"
    );
}

#[test]
fn test_select_nonblocking_default() {
    const SRC: &str = "\
package p

func try(ch chan int) bool {
	select {
	case ch <- 1:
		return true
	default:
		return false
	}
}
";
    let asm = build(SRC, "try");
    assert!(
        asm.contains("select nonblocking"),
        "expected nonblocking select:\n{asm}"
    );
}

#[test]
fn test_compound_assign() {
    const SRC: &str = "\
package p

func addto(n int) int {
	n += 3
	return n
}
";
    let asm = build(SRC, "addto");
    assert!(
        asm.contains("+"),
        "expected += to emit addition:\n{asm}"
    );
}
