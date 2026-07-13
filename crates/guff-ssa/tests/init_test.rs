//! Package-initializer build test (Milestone D, chunk D14).
//!
//! Verifies that `build_package_init` synthesizes the body of a package's
//! `init` function the way go/ssa's `buildPackageInit` does: the `init$guard`
//! re-entry guard (`init.start` / `init.done` blocks), package-level variable
//! initialization in `Info.init_order` (with constant folding of the RHS), and
//! calls to each declared `init` function in source order.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_package_init;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_types::{Checker, Config};

const SRC: &str = "\
package p

var g = 1 + 2

func init() { g = 3 }

func f(x int) int { return x + g }
";

fn build_init_asm() -> String {
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
    build_package_init(&mut prog, ssa_pkg_id, &[file]);

    let init_fid = prog
        .packages
        .get(ssa_pkg_id)
        .init
        .expect("init function synthesized");
    let f = prog.functions.get(init_fid);
    disassemble_function(f, &prog)
}

#[test]
fn test_package_init_disassembly() {
    let asm = build_init_asm();
    println!("{asm}");

    // Header renders as the synthetic parameterless initializer.
    assert!(asm.contains("func init():"), "header:\n{asm}");

    // Entry loads the guard and branches on it.
    assert!(asm.contains("*init$guard"), "expected a guard load:\n{asm}");
    assert!(
        asm.lines().any(|l| l.trim().starts_with("if ")),
        "expected the guard branch:\n{asm}"
    );

    // The two guard blocks are present.
    assert!(asm.contains("init.start"), "missing init.start block:\n{asm}");
    assert!(asm.contains("init.done"), "missing init.done block:\n{asm}");

    // init.start sets the guard true before running the initializers.
    assert!(
        asm.contains("*init$guard = true"),
        "guard should be set true:\n{asm}"
    );

    // The package-level initializer `var g = 1 + 2` is constant-folded to 3.
    assert!(asm.contains("*g = 3"), "global initializer (folded):\n{asm}");

    // The declared `init` function is renamed init#1 and called here.
    assert!(asm.contains("init#1()"), "declared init call:\n{asm}");

    // Control jumps to init.done, whose sole instruction is the return.
    assert!(
        asm.lines().any(|l| l.trim().starts_with("jump ")),
        "expected a jump to init.done:\n{asm}"
    );
    assert!(
        asm.trim_end().ends_with("return"),
        "init.done should end with return:\n{asm}"
    );
}

#[test]
fn test_package_init_bare_inits_has_no_guard() {
    // Under BareInits, go/ssa omits the init$guard machinery entirely: no
    // init.start / init.done blocks, just the initializers and a return.
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", SRC.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);

    let type_pkg_id = check.pkg;
    let mut prog = Program::new(
        BuilderMode::BARE_INITS,
        check.info,
        check.types,
        check.objects,
        check.packages,
    );

    let ssa_pkg_id = create_package(&mut prog, type_pkg_id);
    populate_package_members(&mut prog, ssa_pkg_id, &[file.clone()]);
    build_package_init(&mut prog, ssa_pkg_id, &[file]);

    let init_fid = prog.packages.get(ssa_pkg_id).init.expect("init");
    let asm = disassemble_function(prog.functions.get(init_fid), &prog);
    println!("{asm}");

    assert!(!asm.contains("init$guard"), "BareInits omits the guard:\n{asm}");
    assert!(!asm.contains("init.start"), "BareInits omits guard blocks:\n{asm}");
    // Initializers and declared init calls still run.
    assert!(asm.contains("*g = 3"), "global initializer still runs:\n{asm}");
    assert!(asm.contains("init#1()"), "declared init still called:\n{asm}");
}
