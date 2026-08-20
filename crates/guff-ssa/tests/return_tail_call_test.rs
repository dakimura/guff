//! `return f()` where `f` returns several values returns the *components*, not
//! the tuple — go/ssa's `len(s.Results) == 1 && sig.Results().Len() > 1` case.
//! A consumer reading `Return.results` sees one value per declared result.

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

func two() (int, error) { return 0, nil }

func tail() (int, error) {
	return two()
}

func explicit() (int, error) {
	n, err := two()
	return n, err
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
fn multi_valued_tail_call_returns_its_components() {
    let asm = disassemble("tail");
    // Two extracts, and a return of both — not a return of the call itself.
    assert!(
        asm.contains("extract") && asm.contains("#0") && asm.contains("#1"),
        "expected the tuple to be extracted:\n{asm}"
    );
    let ret = asm
        .lines()
        .find(|l| l.contains("return "))
        .unwrap_or_else(|| panic!("no return in:\n{asm}"));
    assert_eq!(
        ret.matches(',').count(),
        1,
        "return should carry both components: {ret}"
    );
}

#[test]
fn explicit_results_are_unchanged() {
    let asm = disassemble("explicit");
    let ret = asm
        .lines()
        .find(|l| l.contains("return "))
        .unwrap_or_else(|| panic!("no return in:\n{asm}"));
    assert_eq!(ret.matches(',').count(), 1, "{ret}");
}
