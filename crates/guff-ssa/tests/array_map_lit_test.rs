//! Array, slice, and map composite literals (Milestone E, chunk E24).
//!
//! Extends struct composite literals (E23) to the remaining aggregate kinds,
//! mirroring go/ssa's `builder.compLit`:
//!   * an **array** literal fills the aggregate in place through `IndexAddr`
//!     stores (buffered like a struct);
//!   * a **slice** literal allocates a fresh backing array on the heap
//!     (`new [N]T (slicelit)`), fills it, then reslices it (`slice arr[:]`);
//!   * a **map** literal makes a fresh map (`make map[K]V N`) and updates it
//!     with each entry (`m[k] = v`).
//! Elements may be positional or keyed by a constant index (arrays/slices) or
//! by a key expression (maps); a keyed array/slice index resets the running
//! position (`arrayLen`). Outputs are checked against go1.26.4's
//! `ssautil.BuildPackage` (lifted) disassembly (modulo the codebase-wide
//! `:type` constant-suffix difference).

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

func aslit() [3]int          { return [3]int{1, 2, 3} }
func aspartial() [3]int      { return [3]int{7} }
func aslitkey() [4]int       { return [4]int{0: 10, 3: 40} }
func slice() []int           { return []int{1, 2, 3} }
func slicekey() []int        { return []int{2: 9} }
func mp() map[string]int     { return map[string]int{\"a\": 1, \"b\": 2} }
func mpempty() map[string]int { return map[string]int{} }
";

/// A fully-specified array literal writes each element in place. Because the
/// array is filled through the store buffer, all `IndexAddr`s precede all
/// stores, and the aggregate is then loaded and returned.
#[test]
fn test_array_literal() {
    let asm = build("aslit", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = local [3]int (complit)"), "expected array complit local:\n{asm}");
    assert!(asm.contains("t1 = &t0[0]"), "expected &t0[0]:\n{asm}");
    assert!(asm.contains("t2 = &t0[1]"), "expected &t0[1]:\n{asm}");
    assert!(asm.contains("t3 = &t0[2]"), "expected &t0[2]:\n{asm}");
    // All index addresses precede all stores (storebuf ordering).
    let last_addr = asm.find("t3 = &t0[2]").unwrap();
    let first_store = asm.find("*t1 = 1").unwrap();
    assert!(last_addr < first_store, "index addresses must precede stores:\n{asm}");
    assert!(
        asm.contains("*t1 = 1") && asm.contains("*t2 = 2") && asm.contains("*t3 = 3"),
        "expected element stores:\n{asm}"
    );
    assert!(asm.contains("t4 = *t0") && asm.contains("return t4"), "expected load+return:\n{asm}");
}

/// A partial array literal writes only the given prefix; the fresh local is
/// already zero so no memclear is emitted, and untouched elements are absent.
#[test]
fn test_array_partial() {
    let asm = build("aspartial", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t1 = &t0[0]"), "expected &t0[0]:\n{asm}");
    assert!(asm.contains("*t1 = 7"), "expected store of 7:\n{asm}");
    assert!(!asm.contains("&t0[1]") && !asm.contains("&t0[2]"), "must not touch other elements:\n{asm}");
    assert!(asm.contains("t2 = *t0") && asm.contains("return t2"), "expected load+return:\n{asm}");
}

/// Keyed array elements address the given constant indices directly, and the
/// running index is set by each key (here 0 then 3).
#[test]
fn test_array_keyed() {
    let asm = build("aslitkey", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t1 = &t0[0]"), "expected &t0[0]:\n{asm}");
    assert!(asm.contains("t2 = &t0[3]"), "expected &t0[3] (keyed index):\n{asm}");
    assert!(asm.contains("*t1 = 10") && asm.contains("*t2 = 40"), "expected keyed stores:\n{asm}");
    assert!(!asm.contains("&t0[1]") && !asm.contains("&t0[2]"), "must not touch gaps:\n{asm}");
}

/// A slice literal allocates a fresh backing array (`new [3]int (slicelit)`),
/// fills it element by element (interleaved addr/store — no store buffer, since
/// the backing array is unaliased), reslices it, and (after lifting) returns
/// the slice directly.
#[test]
fn test_slice_literal() {
    let asm = build("slice", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = new [3]int (slicelit)"), "expected heap backing array:\n{asm}");
    assert!(asm.contains("t1 = &t0[0]"), "expected &t0[0]:\n{asm}");
    // Interleaved: each store immediately follows its index address.
    let addr1 = asm.find("t1 = &t0[0]").unwrap();
    let store1 = asm.find("*t1 = 1").unwrap();
    let addr2 = asm.find("t2 = &t0[1]").unwrap();
    assert!(addr1 < store1 && store1 < addr2, "slice element stores are interleaved:\n{asm}");
    assert!(asm.contains("*t2 = 2") && asm.contains("*t3 = 3"), "expected element stores:\n{asm}");
    assert!(asm.contains("t4 = slice t0[:]"), "expected reslice:\n{asm}");
    assert!(asm.contains("return t4"), "expected slice returned:\n{asm}");
}

/// A keyed slice literal sizes the backing array to hold the highest index
/// (`arrayLen` = 2+1 = 3) and writes only the keyed element.
#[test]
fn test_slice_keyed() {
    let asm = build("slicekey", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = new [3]int (slicelit)"), "expected size-3 backing array:\n{asm}");
    assert!(asm.contains("t1 = &t0[2]"), "expected &t0[2]:\n{asm}");
    assert!(asm.contains("*t1 = 9"), "expected store of 9:\n{asm}");
    assert!(asm.contains("t2 = slice t0[:]") && asm.contains("return t2"), "expected reslice+return:\n{asm}");
}

/// A map literal makes a fresh map reserving the entry count, then updates it
/// for each entry; after lifting the promoted local disappears and the map is
/// returned directly.
#[test]
fn test_map_literal() {
    let asm = build("mp", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = make map[string]int 2"), "expected make map with reserve 2:\n{asm}");
    assert!(asm.contains("t0[\"a\"] = 1"), "expected map update a:\n{asm}");
    assert!(asm.contains("t0[\"b\"] = 2"), "expected map update b:\n{asm}");
    assert!(asm.contains("return t0"), "expected map returned:\n{asm}");
}

/// An empty map literal reserves zero and does no updates.
#[test]
fn test_map_empty() {
    let asm = build("mpempty", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("t0 = make map[string]int 0"), "expected make map with reserve 0:\n{asm}");
    assert!(!asm.contains("] ="), "empty map has no updates:\n{asm}");
    assert!(asm.contains("return t0"), "expected map returned:\n{asm}");
}

/// Building the whole package with sanity checking enabled exercises the new
/// `IndexAddr`/`Slice`/`MakeMap`/`MapUpdate` instructions against the SSA
/// invariant checker (`sanityCheckPackage`).
#[test]
fn test_sanity_checked() {
    // build() runs build_package, which runs sanity_check over all functions
    // when SANITY_CHECK_FUNCTIONS is set; a violation would panic here.
    let asm = build("slice", BuilderMode::SANITY_CHECK_FUNCTIONS);
    assert!(asm.contains("slice t0[:]"), "asm:\n{asm}");
}
