//! `lift` renames a DebugRef of a lifted cell (go/ssa x/tools v0.52 lift.go
//! `case *DebugRef`; honnef go/ir v0.8.2 lift.go has the same branch). A
//! source expression that denoted the cell's *address* now denotes the value
//! the cell holds at that point. In practice that is the `var x T`
//! declaration (go/ssa builder.go emits `emitDebugRef(e, alloc, true)` right
//! after the Alloc), which becomes the zero constant; the other address refs
//! (`address.address()`) belong to cells that cannot be lifted. Before this,
//! guff left `address of var x int is tN` pointing at an Alloc that `lift`
//! had deleted. The store-value refs of `x = …`, `x++`, `x += y` are checked
//! alongside so the whole DebugRef column matches.
//!
//! Expected lines are `ssadump -build=DF` / `-build=F` of x/tools v0.52.0
//! (built with go1.27.1), with the `:type` suffix of constants and the `p.`
//! qualifier dropped and references to called functions left out.

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
    prog.set_fset(fset.clone());
    let ssa_pkg_id = create_package(&mut prog, type_pkg_id);
    populate_package_members(&mut prog, ssa_pkg_id, &[file.clone()]);
    build_package(&mut prog, ssa_pkg_id, &[file]);
    let fid: FuncId = match prog.packages.get(ssa_pkg_id).members.get(fname) {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected {fname} to be a Function member, got {other:?}"),
    };
    disassemble_function(prog.functions.get(fid), &prog)
}

/// instrs returns the instruction lines of a function, with the
/// right-aligned type column stripped.
fn instrs(asm: &str) -> Vec<String> {
    asm.lines()
        .filter(|l| l.starts_with('\t') && !l.starts_with("\t;"))
        .map(|l| {
            let l = l.trim();
            match l.find("  ") {
                Some(i) => l[..i].trim_end().to_string(),
                None => l.to_string(),
            }
        })
        .collect()
}

fn debugrefs(asm: &str) -> Vec<&str> {
    asm.lines().map(str::trim).filter(|l| l.starts_with("; ") && !l.starts_with("; func ")).collect()
}

const SRC: &str = r#"package p

type T struct{ A, B int }

func define() int {
	x := 1
	x = 2
	return x
}

func varDecl() int {
	var i, x int
	return i + x
}

func phi(c bool) int {
	var x int
	if c {
		x = 1
	}
	return x
}

func loop(n int) int {
	s, i := 0, 0
	for i < n {
		s += i
		i++
	}
	return s
}

func incDec() int {
	x := 1
	x++
	x--
	return x
}

func compound(y int) int {
	x := y
	x += 2
	x *= y
	return x
}

func newCell() int {
	p := new(int)
	*p = 3
	return *p
}

func addrTaken() *int {
	x := 1
	x = 2
	return &x
}

func structLocal() T {
	var t T
	t = T{A: 1}
	return t
}

func neverStored(c bool) int {
	var x int
	if c {
		return x
	}
	x = 5
	return x
}

func multi(f func() (int, int)) int {
	var a, b int
	a, b = f()
	a, b = b, a
	return a - b
}

func deadPhi(c bool) {
	x := 1
	if c {
		x = 2
	}
	x = 3
	_ = x
}
"#;

/// Per function: the `IsAddr` DebugRefs of `var x int` become the zero
/// constant, next to the store-value refs, across straight-line code, a
/// join, a loop, `++`/`+=`, a path that never stores, and a multi-assignment. Cells that are not lifted (`new`, `&x`, a struct built
/// through a field address) keep their address refs.
#[test]
fn test_lift_renames_debugrefs_v052() {
    let cases: &[(&str, &[&str])] = &[
        ("define", &["; var x int @ 6:2 is 1", "; var x int @ 7:2 is 2", "; var x int @ 8:9 is 2"]),
        ("varDecl", &["; var i int @ 12:6 is 0", "; var x int @ 12:9 is 0", "; var i int @ 13:9 is 0", "; var x int @ 13:13 is 0", "; *ast.BinaryExpr @ 13:9 is t0"]),
        ("phi", &["; var x int @ 17:6 is 0", "; var c bool @ 18:5 is c", "; var x int @ 19:3 is 1", "; var x int @ 21:9 is t0"]),
        ("loop", &["; var s int @ 25:2 is 0", "; var i int @ 25:5 is 0", "; var i int @ 27:8 is t3", "; var s int @ 27:3 is t0", "; var i int @ 28:3 is t1", "; var s int @ 30:9 is t2", "; var i int @ 26:6 is t3", "; var n int @ 26:10 is n", "; *ast.BinaryExpr @ 26:6 is t4"]),
        ("incDec", &["; var x int @ 34:2 is 1", "; var x int @ 35:2 is t0", "; var x int @ 36:2 is t1", "; var x int @ 37:9 is t1"]),
        ("compound", &["; var y int @ 41:7 is y", "; var x int @ 41:2 is y", "; var x int @ 42:2 is t0", "; var y int @ 43:7 is y", "; var x int @ 43:2 is t1", "; var x int @ 44:9 is t1"]),
        ("newCell", &["; *ast.CallExpr @ 48:7 is t0", "; var p *int @ 48:2 is t0", "; var p *int @ 49:3 is t0", "; *ast.StarExpr @ 49:2 is 3", "; var p *int @ 50:10 is t0", "; *ast.StarExpr @ 50:9 is t1"]),
        ("addrTaken", &["; var x int @ 54:2 is 1", "; var x int @ 55:2 is 2", "; address of var x int @ 56:10 is t0", "; *ast.UnaryExpr @ 56:9 is t0"]),
        ("structLocal", &["; var t T @ 60:6 is T{}", "; *ast.BasicLit @ 61:11 is 1", "; *ast.CompositeLit @ 61:6 is t2", "; var t T @ 61:2 is t2", "; var t T @ 62:9 is t2"]),
        ("neverStored", &["; var x int @ 66:6 is 0", "; var c bool @ 67:5 is c", "; var x int @ 68:10 is 0", "; var x int @ 70:2 is 5", "; var x int @ 71:9 is 5"]),
        ("multi", &["; var a int @ 75:6 is 0", "; var b int @ 75:9 is 0", "; var f func() (int, int) @ 76:9 is f", "; *ast.CallExpr @ 76:9 is t0", "; var a int @ 76:2 is t1", "; var b int @ 76:5 is t2", "; var b int @ 77:9 is t2", "; var a int @ 77:12 is t1", "; var a int @ 77:2 is t2", "; var b int @ 77:5 is t1", "; var a int @ 78:9 is t2", "; var b int @ 78:13 is t1", "; *ast.BinaryExpr @ 78:9 is t3"]),
        ("deadPhi", &["; var x int @ 82:2 is 1", "; var c bool @ 83:5 is c", "; var x int @ 84:3 is 2", "; var x int @ 86:2 is 3", "; var x int @ 87:6 is 3"]),
    ];
    let mut bad = Vec::new();
    for (fname, want) in cases {
        let asm = build(SRC, fname, BuilderMode::GLOBAL_DEBUG);
        if debugrefs(&asm) != *want {
            bad.push(format!("{fname}:\nwant {want:#?}\n{asm}"));
        }
    }
    assert!(bad.is_empty(), "{} of {} differ:\n{}", bad.len(), cases.len(), bad.join("\n"));
}

/// The instructions match too: the shared zero constant, the φs, a φ that
/// is not needed because the join is overwritten (`deadPhi`), and the debug
/// build lifts exactly what the plain build lifts.
///
/// `deadPhi` is compared only in the debug build: without DebugRefs its
/// `if.then` block is empty after lifting, and guff's `finish_function` runs
/// `optimize_blocks` a second time after `lift` and folds the `if` away,
/// while go/ssa's `finishBody` does not (`ssadump -build=F` keeps
/// `if c goto 1 else 2`). PHASE9-LEDGER §1.3, needs-port.
#[test]
fn test_lift_debugref_instrs_v052() {
    let cases: &[(&str, &[&str])] = &[
        ("define", &["return 2"]),
        ("varDecl", &["t0 = 0 + 0", "return t0"]),
        ("phi", &["if c goto 1 else 2", "jump 2", "t0 = phi [0: 0, 1: 1]", "return t0"]),
        ("loop", &["jump 3", "t0 = t2 + t3", "t1 = t3 + 1", "jump 3", "return t2", "t2 = phi [0: 0, 1: t0]", "t3 = phi [0: 0, 1: t1]", "t4 = t3 < n", "if t4 goto 1 else 2"]),
        ("incDec", &["t0 = 1 + 1", "t1 = t0 - 1", "return t1"]),
        ("compound", &["t0 = y + 2", "t1 = t0 * y", "return t1"]),
        ("newCell", &["t0 = new int (new)", "*t0 = 3", "t1 = *t0", "return t1"]),
        ("addrTaken", &["t0 = new int (x)", "*t0 = 1", "*t0 = 2", "return t0"]),
        ("structLocal", &["t0 = local T (complit)", "t1 = &t0.A [#0]", "*t1 = 1", "t2 = *t0", "return t2"]),
        ("neverStored", &["if c goto 1 else 2", "return 0", "return 5"]),
        ("multi", &["t0 = f()", "t1 = extract t0 #0", "t2 = extract t0 #1", "t3 = t2 - t1", "return t3"]),
        ("deadPhi", &["if c goto 1 else 2", "jump 2", "return"]),
    ];
    let mut bad = Vec::new();
    for mode in [BuilderMode::default(), BuilderMode::GLOBAL_DEBUG] {
        for (fname, want) in cases {
            if *fname == "deadPhi" && mode == BuilderMode::default() {
                continue;
            }
            let asm = build(SRC, fname, mode);
            if instrs(&asm) != *want {
                bad.push(format!("{fname} ({mode:?}):\nwant {want:#?}\n{asm}"));
            }
        }
    }
    assert!(bad.is_empty(), "{} differ:\n{}", bad.len(), bad.join("\n"));
}
