//! Struct field selection `x.f` as an rvalue (Milestone E, chunk E18).
//!
//! A `FieldVal` selector reads a struct field. Because a field of an
//! *addressable* struct (e.g. a value parameter, which is spilled to an
//! addressable cell) is itself addressable, go/ssa's `expr` prefers pointer
//! arithmetic: `&s.f` (FieldAddr) followed by a load, over `Field` subelement
//! extraction. `Field` is only used for a struct held in a register (a
//! non-addressable value). A field through a struct pointer likewise uses
//! FieldAddr + load. Embedded (promoted) fields emit an implicit chain first.

use guff::ast::Decl;
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_function;
use guff_ssa::ids::FuncId;
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_types::{Checker, Config};

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

const SRC: &str = "\
package p

type S struct {
	x int
	y int
}

func f(s S) int { return s.x }
func g(s *S) int { return s.y }
";

/// Field of an addressable struct (value param `s`): the param spills to an
/// addressable cell, so `s.x` is addressable and uses FieldAddr on the cell +
/// load. (Taking the field's address pins the spill, so it survives lifting.)
#[test]
fn test_field_value_naive() {
    let (_p, _f, asm) = build(SRC, "f", BuilderMode::NAIVE_FORM);
    println!("{asm}");
    // FieldAddr computes the address of x (#0), then a load reads it.
    assert!(asm.contains("&t0.x [#0]"), "expected FieldAddr on x:\n{asm}");
}

/// Even after lifting, the receiver spill stays (its field address is taken),
/// so the field read remains FieldAddr + load.
#[test]
fn test_field_value_lifted() {
    let (_p, _f, asm) = build(SRC, "f", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t1 = &t0.x [#0]"), "expected FieldAddr on the spill:\n{asm}");
    assert!(asm.contains("t2 = *t1"), "expected load of the field address:\n{asm}");
    assert!(asm.contains("return t2"), "asm:\n{asm}");
}

/// Field through a struct pointer: FieldAddr then load.
#[test]
fn test_field_pointer_naive() {
    let (_p, _f, asm) = build(SRC, "g", BuilderMode::NAIVE_FORM);
    println!("{asm}");
    assert!(asm.contains("&") && asm.contains(".y [#1]"), "expected FieldAddr on y:\n{asm}");
}

/// Lifted: FieldAddr on the promoted pointer param, then a load.
#[test]
fn test_field_pointer_lifted() {
    let (_p, _f, asm) = build(SRC, "g", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = &s.y [#1]"), "expected FieldAddr on s:\n{asm}");
    assert!(asm.contains("t1 = *t0"), "expected load of the field address:\n{asm}");
    assert!(asm.contains("return t1"), "asm:\n{asm}");
}
