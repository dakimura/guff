//! Named result variables (Milestone E, chunk E17).
//!
//! `createSyntacticParams` allocates a stack local for each named result
//! variable and records it in `Function.named_results`. A `return` then spills
//! its operands into those cells and reloads them to form the returned tuple,
//! so that a naked `return` (and, eventually, deferred functions) observe the
//! latest values. After lifting, these allocs/stores/loads are promoted away
//! and the output matches an unnamed-result function.

use guff::ast::Decl;
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_function;
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_ssa::ids::FuncId;
use guff_types::{Checker, Config};

/// Build `fname` from `src` under `mode` and return the program, its FuncId,
/// and its disassembly.
fn build(src: &str, fname: &str, mode: BuilderMode) -> (Program, FuncId, String) {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", src.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);

    let mut prog = Program::new(mode, check.info, check.types, check.objects, check.packages);
    let type_pkg_id = check.pkg;
    let ssa_pkg_id = guff_ssa::create::create_package(&mut prog, type_pkg_id);

    let fd = file
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::FuncDecl(fd) if fd.name.name == fname => Some(fd),
            _ => None,
        })
        .expect("target FuncDecl not found");

    let fid = guff_ssa::create::create_function(&mut prog, fd.name.name.clone(), None, Some(ssa_pkg_id));
    build_function(&mut prog, fid, fd);

    let asm = disassemble_function(prog.functions.get(fid), &prog);
    (prog, fid, asm)
}

/// Naked return through a named result: `x = 5` stores into the result cell,
/// and the bare `return` reloads it to form the returned value.
#[test]
fn test_naked_return_naive() {
    let src = "package p\nfunc f() (x int) { x = 5; return }";
    let (prog, fid, asm) = build(src, "f", BuilderMode::NAIVE_FORM);
    println!("{asm}");

    // Exactly one named result was recorded.
    assert_eq!(prog.functions.get(fid).named_results.len(), 1, "asm:\n{asm}");

    // The result var is allocated as a local, `x = 5` stores into it, and the
    // naked return reloads it.
    assert!(asm.contains("local int (x)"), "expected result local:\n{asm}");
    assert!(asm.contains("*t0 = 5"), "expected store of 5 into x:\n{asm}");
    assert!(asm.contains("t1 = *t0"), "expected reload of x for return:\n{asm}");
    assert!(asm.contains("return t1"), "expected return of reloaded x:\n{asm}");
}

/// After lifting, the named-result plumbing is promoted away: the naked return
/// yields the constant directly.
#[test]
fn test_naked_return_lifted() {
    let src = "package p\nfunc f() (x int) { x = 5; return }";
    let (_prog, _fid, asm) = build(src, "f", BuilderMode::default());
    println!("{asm}");

    assert!(!asm.contains("local int (x)"), "result local should be lifted away:\n{asm}");
    assert!(asm.contains("return 5"), "expected direct constant return:\n{asm}");
}

/// Explicit return through a named result spills the operand into the result
/// cell and reloads it before returning.
#[test]
fn test_explicit_return_naive() {
    let src = "package p\nfunc g(a int) (x int) { return a }";
    let (prog, fid, asm) = build(src, "g", BuilderMode::NAIVE_FORM);
    println!("{asm}");

    assert_eq!(prog.functions.get(fid).named_results.len(), 1, "asm:\n{asm}");
    // The result var x has its own local, distinct from the spilled param a.
    assert!(asm.contains("local int (a)"), "expected spilled param local:\n{asm}");
    assert!(asm.contains("local int (x)"), "expected result local:\n{asm}");
    // `return a`: load a, store into x, reload x, return the reload.
    assert!(asm.contains("return"), "expected a return:\n{asm}");
}

/// After lifting, `return a` reads the parameter directly.
#[test]
fn test_explicit_return_lifted() {
    let src = "package p\nfunc g(a int) (x int) { return a }";
    let (_prog, _fid, asm) = build(src, "g", BuilderMode::default());
    println!("{asm}");

    assert!(!asm.contains("local int"), "locals should be lifted away:\n{asm}");
    assert!(asm.contains("return a"), "expected direct parameter return:\n{asm}");
}

/// Anonymous result types still get an implicit result local when built from
/// syntax (go: `createSyntacticParams` for `field.Names == nil`).
#[test]
fn test_anonymous_result_gets_implicit_local() {
    let src = "package p\nfunc h(a int) int { return a }";
    let (prog, fid, asm) = build(src, "h", BuilderMode::NAIVE_FORM);
    println!("{asm}");

    assert_eq!(prog.functions.get(fid).named_results.len(), 1, "asm:\n{asm}");
    assert!(asm.contains("return"), "expected a return:\n{asm}");
}
