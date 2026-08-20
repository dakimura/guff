//! A `return` inside a range-over-func body still assigns the enclosing
//! function's results; only the transfer is deferred to the `switch jump {…}`
//! the loop lowers to. go/ssa stores them through
//! `fn.lookup(fn.returnVars[i], false)` before it sets the jump variable, so
//! the values reach the outer function's `Return`.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_package;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_types::{Checker, Config};

const SRC: &str = "\
package p

var errBad error

func chunks(ids []string) func(func([]string) bool) {
	return func(yield func([]string) bool) {}
}

func collect(ids []string) (int, error) {
	n := 0
	for chunk := range chunks(ids) {
		if len(chunk) == 0 {
			return 0, errBad
		}
		n++
	}
	return n, nil
}
";

fn disassemble(want: &str) -> String {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", SRC.as_bytes(), Mode::NONE).expect("parse failed");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    let type_pkg = check.pkg;

    let mut prog = Program::new(
        BuilderMode::default(),
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    let ssa_pkg = create_package(&mut prog, type_pkg);
    populate_package_members(&mut prog, ssa_pkg, &[file.clone()]);
    build_package(&mut prog, ssa_pkg, &[file]);

    let ids: Vec<_> = prog
        .functions
        .iter()
        .filter(|(_, f)| f.name == want)
        .map(|(id, _)| id)
        .collect();
    let id = *ids.first().unwrap_or_else(|| panic!("no function {want}"));
    disassemble_function(prog.functions.get(id), &prog)
}

#[test]
fn yield_closure_stores_the_enclosing_results() {
    let asm = disassemble("collect$1");
    // The `return 0, errBad` arm: two stores, then the jump variable, then
    // `return false`. Without the stores the values would be dropped and the
    // outer function would return whatever its own `return n, nil` left.
    let arm = asm
        .split("if.then")
        .nth(1)
        .unwrap_or_else(|| panic!("no if.then block:\n{asm}"));
    let stores = arm
        .lines()
        .take_while(|l| !l.contains("return false"))
        .filter(|l| l.trim_start().starts_with('*') && l.contains(" = "))
        .count();
    assert!(
        stores >= 3,
        "expected both result stores and the jump store:\n{arm}"
    );
    assert!(arm.contains("return false"), "{arm}");
}

#[test]
fn outer_function_returns_the_stored_results() {
    let asm = disassemble("collect");
    assert!(
        asm.contains("rangefunc.resume.match"),
        "expected the resume switch:\n{asm}"
    );
}
