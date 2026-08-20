//! Range+break CFG tests (unlabeled and labeled).

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_package;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::ids::FuncId;
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_types::{Checker, Config};

fn build(src: &str, fname: &str) -> (Program, FuncId) {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", src.as_bytes(), Mode::NONE).expect("parse");

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
        other => panic!("expected {fname}, got {other:?}"),
    };
    guff_ssa::sanity::sanity_check_function(prog.functions.get(fid));
    (prog, fid)
}

fn build_and_check(src: &str, fname: &str) {
    let _ = build(src, fname);
}

const UNLABELED_SRC: &str = "\
package p

func tick() <-chan int { ch := make(chan int); return ch }

func fn() {
	for range tick() {
		println(\"\")
		if true {
			break
		}
	}
}
";

const LABELED_SRC: &str = "\
package p

func tick() <-chan int { ch := make(chan int); return ch }

func fn() {
outer:
	for range tick() {
		if true {
			break outer
		}
	}
}
";

#[test]
fn range_break_blockopt_preserves_cfg() {
    build_and_check(UNLABELED_SRC, "fn");
}

#[test]
fn labeled_break_from_range_preserves_cfg() {
    build_and_check(LABELED_SRC, "fn");
}

/// A labelled `break` out of an endless loop, from inside a `switch` — the
/// shape that has no other edge into the loop's exit block, so the exit's
/// predecessors are exactly the breaks that reached it.
const LABELED_BREAK_IN_SWITCH: &str = "\
package p

func read() string { return \"\" }

func fn() int {
	n := 0
	line := read()
outer:
	for {
		switch {
		case line == \"end\":
			break outer
		}
		line = read()
		n++
	}
	return n
}
";

#[test]
fn labeled_break_in_switch_leaves_the_loop() {
    let (prog, fid) = build(LABELED_BREAK_IN_SWITCH, "fn");
    let f = prog.functions.get(fid);
    let lb = f.lblocks.get("outer").expect("label outer recorded");
    let done = lb.break_.expect("labelled break target recorded before the body");
    let goto_ = lb.goto_;

    // The loop is endless, so nothing but a `break outer` can reach its exit.
    // While the target was recorded only after the body was built, the break
    // resolved to nothing and `branch_stmt` fell back to the label's goto block
    // — the top of the loop — leaving this block with no predecessor at all.
    // The loop is endless, so nothing but a `break outer` can reach its exit.
    // While the target was recorded only after the body was built, the break
    // resolved to nothing and `branch_stmt` fell back to the label's goto block
    // — the top of the loop — leaving the exit with no predecessor at all.
    // Read `preds` rather than walking `succs`: blockopt fuses a single-pred
    // exit into its predecessor and clears the edge, but the recorded
    // predecessor stays.
    let preds = &f.blocks.get(done).preds;
    assert!(
        !preds.is_empty(),
        "nothing jumps to the labelled break target; the break did not leave the loop"
    );
    assert!(
        !preds.contains(&goto_),
        "the break jumped to the label's own block, not the loop exit"
    );
}
