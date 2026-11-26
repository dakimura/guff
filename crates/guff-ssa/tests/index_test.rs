//! Index expressions `x[i]` as rvalues and lvalues (Milestone E, chunk E22).
//!
//! go/ssa selects the SSA form by the container's index mode and the
//! expression's addressability:
//!   - addressable slice / `*array` (ixVar): `&x[i]` (IndexAddr) + load;
//!   - addressable array in a variable (ixArrVar): `&cell[i]` on the spilled
//!     array's address + load;
//!   - a string or a register array (ixValue / non-addressable ixArrVar):
//!     `x[i]` (Index) extracting the element directly;
//!   - a map (ixMap): `m[k]` (Lookup) on read, `m[k] = v` (MapUpdate) on write.
//!
//! This mirrors go/ssa's addressability dispatch in `builder.expr` plus the
//! `*ast.IndexExpr` cases of `builder.expr0` and `builder.addr`.

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

func sidx(s []int, i int) int          { return s[i] }
func aidx(a [4]int, i int) int         { return a[i] }
func pidx(a *[4]int, i int) int        { return a[i] }
func midx(m map[string]int, k string) int { return m[k] }
func stridx(s string, i int) byte      { return s[i] }
func aset(s []int, i int, v int)       { s[i] = v }
func mset(m map[string]int, k string, v int) { m[k] = v }
";

/// Slice read: an addressable element, so IndexAddr + load. The result type of
/// IndexAddr is `*int`.
#[test]
fn test_slice_index_read() {
    let asm = build("sidx", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = &s[i]"), "expected IndexAddr on slice:\n{asm}");
    assert!(asm.contains("*int"), "expected IndexAddr result type *int:\n{asm}");
    assert!(asm.contains("t1 = *t0"), "expected load of the element address:\n{asm}");
    assert!(asm.contains("return t1"), "asm:\n{asm}");
}

/// Array value parameter: spilled to an addressable cell, so `a[i]` takes the
/// cell's address and uses IndexAddr on it. The spill survives lifting because
/// its element address is taken.
#[test]
fn test_array_index_read() {
    let asm = build("aidx", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = local [4]int (a)"), "expected array spill:\n{asm}");
    assert!(asm.contains("t1 = &t0[i]"), "expected IndexAddr on the spill:\n{asm}");
    assert!(asm.contains("t2 = *t1"), "expected load:\n{asm}");
    assert!(asm.contains("return t2"), "asm:\n{asm}");
}

/// Pointer-to-array: the container is already a pointer, so IndexAddr uses it
/// directly (no spill of the array).
#[test]
fn test_ptr_array_index_read() {
    let asm = build("pidx", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = &a[i]"), "expected IndexAddr on *array:\n{asm}");
    assert!(!asm.contains("local [4]int"), "must not spill the pointee array:\n{asm}");
    assert!(asm.contains("t1 = *t0"), "expected load:\n{asm}");
    assert!(asm.contains("return t1"), "asm:\n{asm}");
}

/// Map read: a Lookup (not IndexAddr). The key already has the map's key type,
/// so no conversion instruction is emitted.
#[test]
fn test_map_index_read() {
    let asm = build("midx", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = m[k]"), "expected map Lookup:\n{asm}");
    assert!(!asm.contains("&m[k]"), "map read must not take an address:\n{asm}");
    assert!(!asm.contains(",ok"), "single-value map read is not comma-ok:\n{asm}");
    assert!(asm.contains("return t0"), "asm:\n{asm}");
}

/// String index: a non-addressable byte value, extracted with Index (not
/// IndexAddr). The element type is `uint8` (byte).
#[test]
fn test_string_index_read() {
    let asm = build("stridx", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = s[i]"), "expected Index on string:\n{asm}");
    assert!(!asm.contains("&s[i]"), "string index has no address:\n{asm}");
    assert!(asm.contains("uint8"), "expected byte (uint8) element type:\n{asm}");
    assert!(asm.contains("return t0"), "asm:\n{asm}");
}

/// Slice element store: IndexAddr then a store through the element address.
#[test]
fn test_slice_index_store() {
    let asm = build("aset", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = &s[i]"), "expected IndexAddr on slice:\n{asm}");
    assert!(asm.contains("*t0 = v"), "expected store through the element address:\n{asm}");
}

/// Map element store: a single MapUpdate `m[k] = v` (no address, no load).
#[test]
fn test_map_index_store() {
    let asm = build("mset", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("m[k] = v"), "expected MapUpdate:\n{asm}");
    assert!(!asm.contains("&m[k]"), "map store takes no address:\n{asm}");
}
