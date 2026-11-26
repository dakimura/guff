//! Comma-ok `exprN` forms (Milestone E, chunk E21): map lookup, channel
//! receive, and type assertion in two-value context.
//!
//! Each of `v, ok := m[k]`, `v, ok := <-ch`, and `v, ok := x.(T)` yields a
//! 2-tuple `(value, ok)`. go/ssa emits a comma-ok `Lookup` / `UnOp <-` /
//! `TypeAssert`, then projects the components with `extract`. The lookup and
//! receive carry the checker's unnamed result tuple (`(int, bool)`); the type
//! test builds a fresh named tuple (`(value int, ok bool)`), matching go/ssa.

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

fn build(src: &str, fname: &str, mode: BuilderMode) -> String {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", src.as_bytes(), Mode::NONE).expect("parse failed");

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

func maplookup(m map[string]int, k string) (int, bool) { v, ok := m[k]; return v, ok }
func recv(ch chan int) (int, bool)                     { v, ok := <-ch; return v, ok }
func assert(x any) (int, bool)                         { v, ok := x.(int); return v, ok }
";

/// Comma-ok map lookup collapses (after lifting) to a single `Lookup` with a
/// `,ok` suffix, whose two components feed the return.
#[test]
fn test_comma_ok_map_lookup() {
    let asm = build(SRC, "maplookup", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("m[k],ok"), "expected comma-ok lookup:\n{asm}");
    // The lookup's type is the checker's unnamed 2-tuple.
    assert!(asm.contains("(int, bool)"), "expected (int, bool) type:\n{asm}");
    assert!(asm.contains("extract t0 #0"), "expected value extract:\n{asm}");
    assert!(asm.contains("extract t0 #1"), "expected ok extract:\n{asm}");
    assert!(asm.contains("return t1, t2"), "expected two-value return:\n{asm}");
}

/// Comma-ok channel receive emits a `<-` unop with the `,ok` suffix.
#[test]
fn test_comma_ok_recv() {
    let asm = build(SRC, "recv", BuilderMode::default());
    println!("{asm}");
    assert!(asm.contains("<-ch,ok"), "expected comma-ok receive:\n{asm}");
    assert!(asm.contains("(int, bool)"), "expected (int, bool) type:\n{asm}");
    assert!(asm.contains("extract t0 #0"), "expected value extract:\n{asm}");
    assert!(asm.contains("extract t0 #1"), "expected ok extract:\n{asm}");
    assert!(asm.contains("return t1, t2"), "expected two-value return:\n{asm}");
}

/// Comma-ok type assertion emits `typeassert,ok`; its fresh result tuple is
/// named `(value int, ok bool)` to match go/ssa's `emitTypeTest`.
#[test]
fn test_comma_ok_type_assert() {
    let asm = build(SRC, "assert", BuilderMode::default());
    println!("{asm}");
    assert!(
        asm.contains("typeassert,ok x.(int)"),
        "expected comma-ok type assertion:\n{asm}"
    );
    assert!(
        asm.contains("(value int, ok bool)"),
        "expected named result tuple:\n{asm}"
    );
    assert!(asm.contains("extract t0 #0"), "expected value extract:\n{asm}");
    assert!(asm.contains("extract t0 #1"), "expected ok extract:\n{asm}");
    assert!(asm.contains("return t1, t2"), "expected two-value return:\n{asm}");
}

/// Naive (unlifted) form keeps the spill cells but still emits the comma-ok
/// instruction and its two extracts.
#[test]
fn test_comma_ok_naive_form() {
    let asm = build(SRC, "maplookup", BuilderMode::NAIVE_FORM);
    println!("{asm}");
    assert!(asm.contains(",ok"), "expected comma-ok suffix in naive form:\n{asm}");
    // Two extracts feed the two result cells.
    assert_eq!(
        asm.matches("extract ").count(),
        2,
        "expected exactly two extracts:\n{asm}"
    );
}
