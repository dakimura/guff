//! Faithful `assignStmt`: storebuf, short var decl (`:=`), parallel and
//! multi-valued assignment (Milestone E, chunk E20).
//!
//! `assignStmt` computes every lvalue before evaluating any RHS, then emits the
//! stores, so a parallel assignment reads the old LHS values. A `:=` creates a
//! fresh local for each newly defined variable; an `a, b = f()` (or
//! `var a, b = f()`) projects the tuple result with `extract`.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_package;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::ids::FuncId;
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_types::{Checker, Config};

/// Build the whole package (so that callees like `pair` are also created) and
/// return the disassembly of the named function.
fn build(src: &str, fname: &str, mode: BuilderMode) -> (Program, FuncId, String) {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", src.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    let type_pkg_id = check.pkg;

    let mut prog = Program::new(mode, check.info, check.types, check.objects, check.packages);
    let ssa_pkg_id = create_package(&mut prog, type_pkg_id);
    populate_package_members(&mut prog, ssa_pkg_id, &[file.clone()]);
    build_package(&mut prog, ssa_pkg_id, &[file]);

    let fid = match prog.packages.get(ssa_pkg_id).members.get(fname) {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected {fname} to be a Function member, got {other:?}"),
    };
    let asm = disassemble_function(prog.functions.get(fid), &prog);
    (prog, fid, asm)
}

const SRC: &str = "\
package p

func pair() (int, int) { return 1, 2 }

func swap(a, b int) (int, int) { a, b = b, a; return a, b }
func multi() int              { x, y := pair(); return x + y }
func def1() int               { z := 5; return z }
func varmulti() int           { var a, b = pair(); return a - b }
func blankrhs()               { _, y := pair(); _ = y }
";

/// Parallel assignment `a, b = b, a`: the storebuf reads the old values, so
/// after lifting the swap collapses to `return b, a`.
#[test]
fn test_parallel_swap() {
    let (_p, _f, asm) = build(SRC, "swap", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("return b, a"), "expected swapped return:\n{asm}");
}

/// Short var decl from a multi-result call: `x, y := pair()` extracts both
/// tuple elements.
#[test]
fn test_short_decl_multi() {
    let (_p, _f, asm) = build(SRC, "multi", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = pair()"), "expected the call:\n{asm}");
    assert!(asm.contains("t1 = extract t0 #0"), "expected extract #0:\n{asm}");
    assert!(asm.contains("t2 = extract t0 #1"), "expected extract #1:\n{asm}");
    assert!(asm.contains("t3 = t1 + t2"), "expected x + y:\n{asm}");
    assert!(asm.contains("return t3"), "asm:\n{asm}");
}

/// Single short var decl `z := 5`: a fresh local, promoted to the constant.
#[test]
fn test_short_decl_single() {
    let (_p, _f, asm) = build(SRC, "def1", BuilderMode::default());
    println!("{asm}");
    // Lifted: z is promoted; the returned value is the constant 5.
    assert!(asm.contains("return 5"), "expected return of 5:\n{asm}");
}

/// `var a, b = pair()` behaves like the short-decl multi case.
#[test]
fn test_var_spec_multi() {
    let (_p, _f, asm) = build(SRC, "varmulti", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = pair()"), "asm:\n{asm}");
    assert!(asm.contains("t1 = extract t0 #0"), "asm:\n{asm}");
    assert!(asm.contains("t2 = extract t0 #1"), "asm:\n{asm}");
    assert!(asm.contains("t3 = t1 - t2"), "asm:\n{asm}");
}

/// A blank LHS in a multi-value assignment `_, y := pair()` still extracts the
/// discarded element (go's assignStmt emits the extract; only the store is a
/// no-op).
#[test]
fn test_blank_lhs_still_extracts() {
    let (_p, _f, asm) = build(SRC, "blankrhs", BuilderMode::NAIVE_FORM);
    println!("{asm}");
    assert!(asm.contains("pair()"), "expected the call:\n{asm}");
    assert!(asm.contains("#0"), "expected extract of the blank element:\n{asm}");
    assert!(asm.contains("#1"), "expected extract of y:\n{asm}");
}
