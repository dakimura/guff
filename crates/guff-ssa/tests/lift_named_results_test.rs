//! A deferred call can assign to a named result, so those cells stay
//! addressable — go/ssa's `liftAlloc` refuses to lift them when `fn.Recover`
//! exists, which is whenever the function defers.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_package;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_types::{Checker, Config};

const SRC: &str = "\
package p

func mu() {}

func withDefer() (err error) {
	err = nil
	defer mu()
	err = nil
	return err
}

func withoutDefer() (err error) {
	err = nil
	err = nil
	return err
}
";

fn disassemble(name: &str) -> String {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", SRC.as_bytes(), Mode::NONE).expect("parse failed");

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

    let fid = match prog.packages.get(ssa_pkg_id).members.get(name) {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected {name} to be a Function member, got {other:?}"),
    };
    disassemble_function(prog.functions.get(fid), &prog)
}

#[test]
fn named_results_stay_spilled_in_a_function_that_defers() {
    let asm = disassemble("withDefer");
    assert!(
        asm.contains("local error (err)"),
        "the named result should keep its cell:\n{asm}"
    );
}

#[test]
fn named_results_are_lifted_without_a_defer() {
    let asm = disassemble("withoutDefer");
    assert!(
        !asm.contains("local error (err)"),
        "the named result should be lifted into registers:\n{asm}"
    );
}
