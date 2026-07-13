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

fn build_and_check(src: &str, fname: &str) {
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
