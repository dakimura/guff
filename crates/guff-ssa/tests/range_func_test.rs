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

fn build_prog(src: &str) -> (Program, FuncId) {
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

    let fid: FuncId = match prog.packages.get(ssa_pkg_id).members.get("use") {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected use, got {other:?}"),
    };
    (prog, fid)
}

fn build(src: &str) -> String {
    let (prog, fid) = build_prog(src);
    disassemble_function(prog.functions.get(fid), &prog)
}

fn build_yield(src: &str) -> String {
    let (prog, fid) = build_prog(src);
    let yid = prog.functions.get(fid).anon_funcs[0];
    disassemble_function(prog.functions.get(yid), &prog)
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
    let asm = build(SRC);
    assert!(
        asm.contains("make closure"),
        "expected MakeClosure for yield function:\n{asm}"
    );
    assert!(
        asm.contains("rangefunc"),
        "expected rangefunc blocks:\n{asm}"
    );
    let yield_asm = build_yield(SRC);
    assert!(
        yield_asm.contains("yield-loop") || yield_asm.contains("+="),
        "yield body must remain reachable (entry before yield-continue):\n{yield_asm}"
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
    let asm = build(SRC);
    assert!(
        asm.contains("rangefunc"),
        "expected rangefunc lowering:\n{asm}"
    );
}

/// Parent function has many blocks before the range-over-func so the parent's
/// `rangefunc.done` BlockId is past the yield function's blocks arena length.
/// Regression for looking up a foreign BlockId in the yield arena (OOB panic).
#[test]
fn range_over_func_break_with_many_parent_blocks() {
    const SRC: &str = "\
package p

func Seq(yield func(int) bool) {}

func use(x int) {
	switch x {
	case 0:
		_ = 0
	case 1:
		_ = 1
	case 2:
		_ = 2
	case 3:
		_ = 3
	case 4:
		_ = 4
	case 5:
		_ = 5
	case 6:
		_ = 6
	case 7:
		_ = 7
	case 8:
		_ = 8
	case 9:
		_ = 9
	case 10:
		_ = 10
	case 11:
		_ = 11
	case 12:
		_ = 12
	case 13:
		_ = 13
	case 14:
		_ = 14
	case 15:
		_ = 15
	case 16:
		_ = 16
	case 17:
		_ = 17
	case 18:
		_ = 18
	case 19:
		_ = 19
	}
	for k := range Seq {
		if k == 3 {
			break
		}
	}
}
";
    let asm = build(SRC);
    assert!(
        asm.contains("rangefunc"),
        "expected rangefunc lowering without panic:\n{asm}"
    );
}

/// Yield CFG entry must be allocated before `yield-continue`, otherwise
/// blockopt treats continue as the root and deletes the whole body — which
/// also caused SA4017 false positives on used pure-call results in the body.
#[test]
fn range_over_func_body_not_deleted() {
    const SRC: &str = "\
package p

func Seq(yield func(string) bool) {}
func use(s string) {
	for dp := range Seq {
		dp = s
		_ = dp
	}
}
";
    let yield_asm = build_yield(SRC);
    assert!(
        yield_asm.contains("entry") || yield_asm.contains("*s") || yield_asm.contains("FreeVar"),
        "yield body must stay reachable from CFG entry:\n{yield_asm}"
    );
    // Before the fix, only yield-continue survived (ready + return true).
    assert!(
        yield_asm.lines().any(|l| l.contains("return") && !l.contains("true"))
            || yield_asm.matches("return").count() >= 1
                && (yield_asm.contains("yield-loop")
                    || yield_asm.contains("Busy")
                    || yield_asm.contains("-1")
                    || yield_asm.contains("*s")),
        "expected yield body beyond bare continue:\n{yield_asm}"
    );
}
