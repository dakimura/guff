//! Struct field selection `x.f` as an lvalue (Milestone E, chunk E19).
//!
//! The `*ast.SelectorExpr` case of `builder.addr` builds a `LazyAddress` whose
//! deferred computation is a `FieldAddr`: the receiver `x` is emitted eagerly
//! (its address for a value receiver, its loaded pointer for a `*struct`
//! receiver), and the field address is emitted at store/address time. This
//! covers `x.f = v` (assignment) and `&x.f` (address-of).

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

func setv(s S, v int)  { s.x = v }
func setp(s *S, v int) { s.y = v }
func addrp(s *S) *int  { return &s.x }
";

/// Assigning through a struct *value* receiver: the receiver spill's address is
/// taken directly, so a `FieldAddr` targets it (no load of the whole struct).
/// The spill cannot be register-promoted because its address escapes into the
/// FieldAddr, so `local S (s)` survives lifting.
#[test]
fn test_field_assign_value_receiver() {
    let (_p, _f, asm) = build(SRC, "setv", BuilderMode::default());
    println!("{asm}");
    // s stays in memory; the field address is taken on the spill cell.
    assert!(asm.contains("local S (s)"), "spill of s should survive:\n{asm}");
    assert!(asm.contains("&t0.x [#0]"), "expected FieldAddr on spill:\n{asm}");
    assert!(asm.contains("*t1 = v"), "expected store of v into &s.x:\n{asm}");
}

/// Assigning through a struct *pointer* receiver: the pointer param is loaded
/// (or, after lifting, used directly), then a `FieldAddr` targets the pointee.
#[test]
fn test_field_assign_pointer_receiver_lifted() {
    let (_p, _f, asm) = build(SRC, "setp", BuilderMode::default());
    println!("{asm}");
    // Lifted: the pointer param is promoted, FieldAddr targets it directly.
    assert!(asm.contains("t0 = &s.y [#1]"), "expected FieldAddr on s:\n{asm}");
    assert!(asm.contains("*t0 = v"), "expected store of v into &s.y:\n{asm}");
}

/// Naive form of the pointer-receiver assignment: the param is spilled, loaded,
/// and the field address is emitted lazily at store time.
#[test]
fn test_field_assign_pointer_receiver_naive() {
    let (_p, _f, asm) = build(SRC, "setp", BuilderMode::NAIVE_FORM);
    println!("{asm}");
    // The pointer is loaded out of its spill before the field address is taken.
    assert!(asm.contains("local *S (s)"), "expected spill of pointer param:\n{asm}");
    assert!(asm.contains(".y [#1]"), "expected FieldAddr on field y:\n{asm}");
    assert!(asm.contains("&"), "field address must be taken:\n{asm}");
}

/// Address-of a struct-pointer field `&s.x`: the `&` unary operator routes to
/// the SelectorExpr lvalue and returns the field address without a load.
#[test]
fn test_addr_of_field() {
    let (_p, _f, asm) = build(SRC, "addrp", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = &s.x [#0]"), "expected FieldAddr on s.x:\n{asm}");
    assert!(asm.contains("return t0"), "expected the address to be returned:\n{asm}");
    // No load: the address itself is the value, not the field's contents.
    assert!(!asm.contains("= *t0"), "must not load the field:\n{asm}");
}
