//! End-to-end build orchestration test (Milestone D, chunk D12).
//!
//! Drives the full pipeline for a package-level function: CREATE-phase member
//! population (which allocates the Function with its signature and object) then
//! `build_function` (params + body + post-construction passes), and checks the
//! disassembly.

use guff::ast::Decl;
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_function;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_types::{Checker, Config};

const SRC: &str = "\
package p

func f(a int, b int) int {
	return a + b
}
";

#[test]
fn test_build_function_end_to_end() {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", SRC.as_bytes(), Mode::NONE).expect("parse failed");

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

    // populate_package_members created f (with signature + object); look it up.
    let fn_id = match prog.packages.get(ssa_pkg_id).members.get("f") {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected f to be a Function member, got {other:?}"),
    };
    assert!(
        prog.functions.get(fn_id).signature.is_some(),
        "member population should have set f's signature"
    );

    let fd = file
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::FuncDecl(fd) if fd.name.name == "f" => Some(fd),
            _ => None,
        })
        .unwrap();

    build_function(&mut prog, fn_id, fd);

    let f = prog.functions.get(fn_id);
    // A body was built.
    assert!(!f.blocks.is_empty(), "f should have basic blocks after build");
    // Both parameters were created from the signature.
    assert_eq!(f.params.len(), 2, "f has two parameters");

    let asm = disassemble_function(f, &prog);
    assert!(asm.contains("func f(a int, b int) int:"), "header:\n{asm}");
    assert!(asm.contains("a + b"), "expected the add in:\n{asm}");
    assert!(asm.contains("return"), "expected a return in:\n{asm}");
}

const SRC_BRANCH: &str = "\
package p

func g(x int) int {
	if x > 0 {
		return x
	}
	return 0
}
";

/// A function with an early return in a branch: `if x > 0 { return x }` is
/// followed by `return 0`. The statement after the (terminated) then-branch
/// must still build — go/ssa routes it through an unreachable block that block
/// optimization removes (regression for the D13 fix).
#[test]
fn test_build_function_early_return_in_branch() {
    let fset = FileSet::new();
    let file = parse_file(&fset, "g.go", SRC_BRANCH.as_bytes(), Mode::NONE).expect("parse failed");

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

    let fn_id = match prog.packages.get(ssa_pkg_id).members.get("g") {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected g to be a Function member, got {other:?}"),
    };
    let fd = file
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::FuncDecl(fd) if fd.name.name == "g" => Some(fd),
            _ => None,
        })
        .unwrap();

    build_function(&mut prog, fn_id, fd);

    let f = prog.functions.get(fn_id);
    let asm = disassemble_function(f, &prog);
    assert!(asm.contains("func g(x int) int:"), "header:\n{asm}");
    assert!(asm.contains("if "), "expected a branch in:\n{asm}");
    // Both returns are present; the unreachable blocks were cleaned up.
    assert_eq!(asm.matches("return").count(), 2, "expected two returns in:\n{asm}");
    assert!(!asm.contains("unreachable"), "unreachable blocks should be removed:\n{asm}");
}
