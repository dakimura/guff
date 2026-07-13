//! Whole-package build orchestration test (Milestone D, chunk D15).
//!
//! Drives `build_package`, the sequential analog of go/ssa's `(*Package).build`
//! / `(*builder).iterate`: from a type-checked package with declared functions,
//! a declared `init`, and a package-level variable, a single call must build
//! every declared function's body *and* the synthesized package initializer.

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

var g = 1 + 2

func init() { g = 3 }

func f(a int, b int) int {
	return a + b
}

func h(x int) int {
	return x * 2
}
";

/// A single `build_package` call builds `f`, `h`, and the synthesized `init`
/// (which also runs the declared `init`, renamed `init#1`, and the package-level
/// initializer for `g`).
#[test]
fn test_build_package_builds_all_functions() {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", SRC.as_bytes(), Mode::NONE).expect("parse failed");

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

    // Single orchestration call builds every function of the package.
    build_package(&mut prog, ssa_pkg_id, &[file]);

    // Both declared functions have bodies.
    for name in ["f", "h"] {
        let fid = match prog.packages.get(ssa_pkg_id).members.get(name) {
            Some(MemberData::Function(fid)) => *fid,
            other => panic!("expected {name} to be a Function member, got {other:?}"),
        };
        let f = prog.functions.get(fid);
        assert!(
            !f.blocks.is_empty(),
            "{name} should have basic blocks after build_package"
        );
    }

    // f's disassembly is the same as building it in isolation.
    let f_fid = match prog.packages.get(ssa_pkg_id).members.get("f") {
        Some(MemberData::Function(fid)) => *fid,
        _ => unreachable!(),
    };
    let f_asm = disassemble_function(prog.functions.get(f_fid), &prog);
    assert!(f_asm.contains("func f(a int, b int) int:"), "f header:\n{f_asm}");
    assert!(f_asm.contains("a + b"), "expected the add in f:\n{f_asm}");

    // h's disassembly built as part of the same call.
    let h_fid = match prog.packages.get(ssa_pkg_id).members.get("h") {
        Some(MemberData::Function(fid)) => *fid,
        _ => unreachable!(),
    };
    let h_asm = disassemble_function(prog.functions.get(h_fid), &prog);
    assert!(h_asm.contains("func h(x int) int:"), "h header:\n{h_asm}");

    // The synthesized initializer was built too: guard, folded global init, and
    // the call to the declared init (renamed init#1).
    let init_fid = prog
        .packages
        .get(ssa_pkg_id)
        .init
        .expect("init synthesized");
    let init_asm = disassemble_function(prog.functions.get(init_fid), &prog);
    assert!(init_asm.contains("func init():"), "init header:\n{init_asm}");
    assert!(init_asm.contains("*g = 3"), "folded global init:\n{init_asm}");
    assert!(init_asm.contains("init#1()"), "declared init call:\n{init_asm}");
}
