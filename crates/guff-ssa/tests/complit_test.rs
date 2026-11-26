//! Struct composite literals `T{…}` (Milestone E, chunk E23).
//!
//! A composite literal is built into fresh storage: a stack local (`local T
//! (complit)`) for a value literal, or a heap allocation (`new T (complit)`)
//! when its address escapes (`&T{…}`). Each field is written through a
//! `FieldAddr`; go/ssa buffers the stores so all field addresses are computed
//! before any store (matching a `storebuf`). A value literal is then loaded to
//! yield the aggregate. An empty literal `T{}` lifts to the zero constant
//! `T{}`. This mirrors go/ssa's `builder.compLit` (struct case) plus the
//! `*ast.CompositeLit` cases of `builder.addr`/`expr0`.

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

fn build(fname: &str, mode: BuilderMode) -> String {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", SRC.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    assert!(check.errors.is_empty(), "type errors: {:?}", check.errors);
    let type_pkg_id = check.pkg;

    let mut prog = Program::new(mode, check.info, check.types, check.objects, check.packages);
    let ssa_pkg_id = create_package(&mut prog, type_pkg_id);
    populate_package_members(&mut prog, ssa_pkg_id, &[file.clone()]);
    build_package(&mut prog, ssa_pkg_id, &[file]);

    let fid: FuncId = match prog.packages.get(ssa_pkg_id).members.get(fname) {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected {fname} to be a Function member, got {other:?}"),
    };
    disassemble_function(prog.functions.get(fid), &prog)
}

const SRC: &str = "\
package p

type S struct {
	x int
	y int
}

func full() S    { return S{x: 1, y: 2} }
func positional() S { return S{1, 2} }
func partial() S { return S{x: 1} }
func keyed() S   { return S{y: 5} }
func empty() S   { return S{} }
func ptr() *S    { return &S{x: 7} }
";

/// A fully-specified struct literal writes every field. All FieldAddrs are
/// emitted before the stores (storebuf ordering), then the aggregate is loaded.
#[test]
fn test_full_struct_literal() {
    let asm = build("full", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = local S (complit)"), "expected complit local:\n{asm}");
    assert!(asm.contains("t1 = &t0.x [#0]"), "expected FieldAddr x:\n{asm}");
    assert!(asm.contains("t2 = &t0.y [#1]"), "expected FieldAddr y:\n{asm}");
    // Both field addresses precede both stores.
    let ix = asm.find("t2 = &t0.y").unwrap();
    let store_x = asm.find("*t1 = 1").unwrap();
    assert!(ix < store_x, "field addresses must precede stores:\n{asm}");
    assert!(asm.contains("*t1 = 1") && asm.contains("*t2 = 2"), "expected stores:\n{asm}");
    assert!(asm.contains("t3 = *t0") && asm.contains("return t3"), "expected load+return:\n{asm}");
}

/// Positional elements initialize fields in order.
#[test]
fn test_positional_struct_literal() {
    let asm = build("positional", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("&t0.x [#0]") && asm.contains("&t0.y [#1]"), "expected both fields:\n{asm}");
    assert!(asm.contains("*t1 = 1") && asm.contains("*t2 = 2"), "expected ordered stores:\n{asm}");
}

/// A partial literal writes only the named field; the rest stay zero (the fresh
/// local is already zero, so no memclear is emitted).
#[test]
fn test_partial_struct_literal() {
    let asm = build("partial", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t1 = &t0.x [#0]"), "expected FieldAddr x:\n{asm}");
    assert!(asm.contains("*t1 = 1"), "expected store to x:\n{asm}");
    assert!(!asm.contains("[#1]"), "must not touch the unset field y:\n{asm}");
    assert!(asm.contains("return t2"), "asm:\n{asm}");
}

/// A keyed literal addresses the named field by its declared index (`y` = #1).
#[test]
fn test_keyed_struct_literal() {
    let asm = build("keyed", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t1 = &t0.y [#1]"), "expected FieldAddr y (#1):\n{asm}");
    assert!(asm.contains("*t1 = 5"), "expected store to y:\n{asm}");
    assert!(!asm.contains("[#0]"), "must not touch the unset field x:\n{asm}");
}

/// An empty literal has no stores; lifting promotes the never-written local to
/// the zero constant `S{}`.
#[test]
fn test_empty_struct_literal() {
    let asm = build("empty", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("return S{}"), "expected zero-value constant:\n{asm}");
    assert!(!asm.contains("local S"), "empty literal should be promoted away:\n{asm}");
}

/// `&S{…}` allocates on the heap (`new`), fills it, and yields the pointer
/// directly (no load). Requires the checker's composite-literal addressability
/// exception (`&CompositeLit`).
#[test]
fn test_pointer_struct_literal() {
    let asm = build("ptr", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = new S (complit)"), "expected heap alloc:\n{asm}");
    assert!(asm.contains("t1 = &t0.x [#0]"), "expected FieldAddr x:\n{asm}");
    assert!(asm.contains("*t1 = 7"), "expected store:\n{asm}");
    assert!(asm.contains("return t0"), "expected the pointer returned directly:\n{asm}");
    assert!(!asm.contains("= *t0"), "pointer literal must not load the aggregate:\n{asm}");
}
