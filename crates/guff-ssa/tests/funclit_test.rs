//! Anonymous-function (FuncLit) descent test (Milestone D, chunk D16).
//!
//! A non-capturing function literal must be created as an anonymous
//! [`Function`] enclosed by the function that contains it, recorded in the
//! parent's `anon_funcs`, and built eagerly. Because it captures no outer
//! variables, the literal evaluates directly to a function value (go/ssa returns
//! the bare `*Function` when `anon.FreeVars == nil`).

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_package;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_types::{Checker, Config};

// An immediately-invoked function literal that captures nothing.
const SRC: &str = "\
package p

func f() int {
	return func() int { return 42 }()
}
";

#[test]
fn test_non_capturing_funclit() {
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
    build_package(&mut prog, ssa_pkg_id, &[file]);

    let f_fid = match prog.packages.get(ssa_pkg_id).members.get("f") {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected f to be a Function member, got {other:?}"),
    };

    // f recorded exactly one anonymous function, named "f$1".
    let anon_fids = prog.functions.get(f_fid).anon_funcs.clone();
    assert_eq!(anon_fids.len(), 1, "f should enclose one function literal");
    let anon_fid = anon_fids[0];
    let anon = prog.functions.get(anon_fid);
    assert_eq!(anon.name, "f$1", "anonymous function naming");
    assert_eq!(anon.parent, Some(f_fid), "anon's parent is f");
    assert!(anon.freevars.is_empty(), "the literal captures nothing");
    assert!(!anon.blocks.is_empty(), "the literal's body was built");

    // The anonymous function returns the constant 42.
    let anon_asm = disassemble_function(anon, &prog);
    println!("--- anon ---\n{anon_asm}");
    assert!(anon_asm.contains("return"), "anon body:\n{anon_asm}");

    // f calls the literal directly (a Call whose callee is the anon function),
    // so its disassembly references the anon function's name.
    let f_asm = disassemble_function(prog.functions.get(f_fid), &prog);
    println!("--- f ---\n{f_asm}");
    assert!(f_asm.contains("func f() int:"), "f header:\n{f_asm}");
    assert!(f_asm.contains("f$1"), "f should call the literal f$1:\n{f_asm}");
}

// A function literal that captures the enclosing function's parameter `x`.
const SRC_CAPTURE: &str = "\
package p

func adder(x int) func() int {
	return func() int { return x }
}
";

#[test]
fn test_capturing_funclit_emits_make_closure() {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", SRC_CAPTURE.as_bytes(), Mode::NONE).expect("parse failed");

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

    let adder_fid = match prog.packages.get(ssa_pkg_id).members.get("adder") {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected adder to be a Function member, got {other:?}"),
    };
    let anon_fid = prog.functions.get(adder_fid).anon_funcs[0];

    // The anonymous function captured exactly one free variable, `x`.
    let anon = prog.functions.get(anon_fid);
    assert_eq!(anon.freevars.len(), 1, "the literal captures one variable");
    let (_, fv) = anon.freevars.iter().next().unwrap();
    assert_eq!(fv.name, "x", "captured free variable name");

    // Reference capture (go/ssa faithful, resolving the D17 value-capture
    // divergence): `x` is spilled to a cell, and the closure captures that
    // *address*. So adder spills x, then `make closure adder$1 [<spill addr>]`.
    //
    // `new`, not `local`: the capture reaches the cell through `lookup(…,
    // escaping = true)`, which heap-allocates it. Measured against x/tools
    // v0.48.0 `go/ssa` in NaiveForm on this exact source — it prints
    // `t0 = new int (x)` and `adder`'s `Locals` does not contain `x` at all.
    let adder_asm = disassemble_function(prog.functions.get(adder_fid), &prog);
    println!("--- adder ---\n{adder_asm}");
    assert!(adder_asm.contains("new int (x)"), "x is spilled to a heap cell:\n{adder_asm}");
    assert!(
        !prog
            .functions
            .get(adder_fid)
            .locals
            .iter()
            .any(|&id| matches!(prog.functions.get(adder_fid).instrs.get(id),
                guff_ssa::instr::InstrData::Alloc(a) if a.comment == "x")),
        "an escaping cell is not a local"
    );
    assert!(adder_asm.contains("*t0 = x"), "param stored to its cell:\n{adder_asm}");
    // The closure binds the spill cell (a register), not the bare param value.
    assert!(
        adder_asm.contains("make closure adder$1 [t0]"),
        "MakeClosure binds the spill address:\n{adder_asm}"
    );

    // The closure body loads the value through the captured free variable `x`
    // (which now holds the `*int` spill address).
    let (_, fv) = prog.functions.get(anon_fid).freevars.iter().next().unwrap();
    assert_eq!(fv.name, "x", "captured free variable name");
    let anon_asm = disassemble_function(prog.functions.get(anon_fid), &prog);
    println!("--- adder$1 ---\n{anon_asm}");
    assert!(anon_asm.contains("*x"), "closure loads through freevar x:\n{anon_asm}");
}
