//! `for k, v = range x` in go/ssa (x/tools v0.52) and honnef's go/ir
//! v0.8.2: `rangeStmt` gets the key and value from `rangeIndexed` /
//! `rangeIter` first, then declares the `:=` variables (go1.22+), then
//! evaluates *both* lvalues ("Evaluate both LHS expressions before we update
//! either"), and only then stores the key and the value.

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
    build_nth(src, fname, mode, None)
}

/// build_nth disassembles `fname`, or its `anon`th anonymous function.
fn build_nth(src: &str, fname: &str, mode: BuilderMode, anon: Option<usize>) -> String {
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
    let fid = match anon {
        Some(i) => prog.functions.get(fid).anon_funcs[i],
        None => fid,
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

const SRC: &str = r#"package p

type T struct {
	A, B int
	R    rune
}

var sink func()

func sliceAssign(q *T, s []int) {
	for q.A, q.B = range s {
	}
}
func arrAssign(q *T, a [3]string, m map[int]string) {
	for q.A, m[q.B] = range a {
	}
}
func ptrArrAssign(q *T, a *[3]int) {
	for q.A, q.B = range a {
	}
}
func strAssign(q *T, s string) {
	for q.A, q.R = range s {
	}
}
func mapAssign(q *T, m map[int]int) {
	for q.A, q.B = range m {
	}
}
func keyOnly(q *T, s []int) {
	for q.A = range s {
	}
}
func valueOnly(q *T, s []int) {
	for _, q.B = range s {
	}
}
func define(s []int) int {
	n := 0
	for i, x := range s {
		n += i + x
	}
	return n
}
func defineEscape(s []int) {
	for i, x := range s {
		sink = func() { _ = i + x }
	}
}
func defineMapEscape(m map[string]int) {
	for k, v := range m {
		sink = func() { _ = k; _ = v }
	}
}
func defineStrEscape(s string) {
	for i, r := range s {
		sink = func() { _, _ = i, r }
	}
}
func localAssign(s []int) int {
	var i, x int
	for i, x = range s {
	}
	return i + x
}
func idxAssign(a []int, s []int) {
	for a[0], a[1] = range s {
	}
}

func rfAssign(q *T, m map[int]int, seq func(yield func(int, int) bool)) {
	for q.A, m[q.B] = range seq {
	}
}
"#;

/// Each expectation is `ssadump -build=F` of golang.org/x/tools v0.52.0
/// built with go1.27.1, type column, constant `:type` suffixes and the phi
/// comments (`#rangeindex`) dropped. `defineStrEscape` (`for i, r := range s`
/// with both captured) also has the new order but is not listed: guff's
/// printer spells the `rune` alias `int32`.
#[test]
fn test_range_assign_instrs_v052() {
    let cases: &[(&str, &[&str])] = &[
        ("sliceAssign", &["t0 = len(s)", "jump 1", "t1 = phi [0: -1, 2: t2]", "t2 = t1 + 1", "t3 = t2 < t0", "if t3 goto 2 else 3", "t4 = &s[t2]", "t5 = *t4", "t6 = &q.A [#0]", "*t6 = t2", "t7 = &q.B [#1]", "*t7 = t5", "jump 1", "return"]),
        ("arrAssign", &["jump 1", "t0 = phi [0: -1, 2: t1]", "t1 = t0 + 1", "t2 = t1 < 3", "if t2 goto 2 else 3", "t3 = a[t1]", "t4 = &q.B [#1]", "t5 = *t4", "t6 = &q.A [#0]", "*t6 = t1", "m[t5] = t3", "jump 1", "return"]),
        ("ptrArrAssign", &["jump 1", "t0 = phi [0: -1, 2: t1]", "t1 = t0 + 1", "t2 = t1 < 3", "if t2 goto 2 else 3", "t3 = &a[t1]", "t4 = *t3", "t5 = &q.A [#0]", "*t5 = t1", "t6 = &q.B [#1]", "*t6 = t4", "jump 1", "return"]),
        ("strAssign", &["t0 = range s", "jump 1", "t1 = next t0", "t2 = extract t1 #0", "if t2 goto 2 else 3", "t3 = extract t1 #1", "t4 = extract t1 #2", "t5 = &q.A [#0]", "*t5 = t3", "t6 = &q.R [#2]", "*t6 = t4", "jump 1", "return"]),
        ("mapAssign", &["t0 = range m", "jump 1", "t1 = next t0", "t2 = extract t1 #0", "if t2 goto 2 else 3", "t3 = extract t1 #1", "t4 = extract t1 #2", "t5 = &q.A [#0]", "*t5 = t3", "t6 = &q.B [#1]", "*t6 = t4", "jump 1", "return"]),
        ("keyOnly", &["t0 = len(s)", "jump 1", "t1 = phi [0: -1, 2: t2]", "t2 = t1 + 1", "t3 = t2 < t0", "if t3 goto 2 else 3", "t4 = &q.A [#0]", "*t4 = t2", "jump 1", "return"]),
        ("valueOnly", &["t0 = len(s)", "jump 1", "t1 = phi [0: -1, 2: t2]", "t2 = t1 + 1", "t3 = t2 < t0", "if t3 goto 2 else 3", "t4 = &s[t2]", "t5 = *t4", "t6 = &q.B [#1]", "*t6 = t5", "jump 1", "return"]),
        ("define", &["t0 = len(s)", "jump 1", "t1 = phi [0: 0, 2: t8]", "t2 = phi [0: -1, 2: t3]", "t3 = t2 + 1", "t4 = t3 < t0", "if t4 goto 2 else 3", "t5 = &s[t3]", "t6 = *t5", "t7 = t3 + t6", "t8 = t1 + t7", "jump 1", "return t1"]),
        ("defineEscape", &["t0 = len(s)", "jump 1", "t1 = phi [0: -1, 2: t2]", "t2 = t1 + 1", "t3 = t2 < t0", "if t3 goto 2 else 3", "t4 = &s[t2]", "t5 = *t4", "t6 = new int (i)", "t7 = new int (x)", "*t6 = t2", "*t7 = t5", "t8 = make closure defineEscape$1 [t6, t7]", "*sink = t8", "jump 1", "return"]),
        ("defineMapEscape", &["t0 = range m", "jump 1", "t1 = next t0", "t2 = extract t1 #0", "if t2 goto 2 else 3", "t3 = extract t1 #1", "t4 = extract t1 #2", "t5 = new string (k)", "t6 = new int (v)", "*t5 = t3", "*t6 = t4", "t7 = make closure defineMapEscape$1 [t5, t6]", "*sink = t7", "jump 1", "return"]),
        ("localAssign", &["t0 = len(s)", "jump 1", "t1 = phi [0: 0, 2: t4]", "t2 = phi [0: 0, 2: t7]", "t3 = phi [0: -1, 2: t4]", "t4 = t3 + 1", "t5 = t4 < t0", "if t5 goto 2 else 3", "t6 = &s[t4]", "t7 = *t6", "jump 1", "t8 = t1 + t2", "return t8"]),
        ("idxAssign", &["t0 = len(s)", "jump 1", "t1 = phi [0: -1, 2: t2]", "t2 = t1 + 1", "t3 = t2 < t0", "if t3 goto 2 else 3", "t4 = &s[t2]", "t5 = *t4", "t6 = &a[0]", "*t6 = t2", "t7 = &a[1]", "*t7 = t5", "jump 1", "return"]),
    ];
    let mut bad = Vec::new();
    for (fname, want) in cases {
        let asm = build(SRC, fname, BuilderMode::default());
        if instrs(&asm) != *want {
            bad.push(format!("{fname}:\nwant {want:#?}\n{asm}"));
        }
    }
    assert!(bad.is_empty(), "{} of {} differ:\n{}", bad.len(), cases.len(), bad.join("\n"));
}

/// The DebugRef lines of `ssadump -build=DF` for the same functions, minus
/// the references to called functions, package qualifier dropped.
/// `localAssign` is left out: its `var i, x int` refs differ because guff's
/// `lift` does not rename an `IsAddr` DebugRef of a lifted Alloc to the
/// cell's value (go/ssa lift.go `case *DebugRef`; PHASE9-LEDGER
/// xtools-substrate, needs-port) — unrelated to the range statement.
#[test]
fn test_range_assign_debugrefs_v052() {
    let cases: &[(&str, &[&str])] = &[
        ("sliceAssign", &["; var s []int @ 11:23 is s", "; var q *T @ 11:6 is q", "; var q *T @ 11:11 is q", "; address of field A int @ 11:8 is t6", "; field A int @ 11:8 is t2", "; address of field B int @ 11:13 is t7", "; field B int @ 11:13 is t5"]),
        ("arrAssign", &["; var a [3]string @ 15:26 is a", "; var q *T @ 15:6 is q", "; var m map[int]string @ 15:11 is m", "; var q *T @ 15:13 is q", "; address of field B int @ 15:15 is t4", "; *ast.SelectorExpr @ 15:13 is t5", "; address of field A int @ 15:8 is t6", "; field A int @ 15:8 is t1"]),
        ("ptrArrAssign", &["; var a *[3]int @ 19:23 is a", "; var q *T @ 19:6 is q", "; var q *T @ 19:11 is q", "; address of field A int @ 19:8 is t5", "; field A int @ 19:8 is t1", "; address of field B int @ 19:13 is t6", "; field B int @ 19:13 is t4"]),
        ("strAssign", &["; var s string @ 23:23 is s", "; var q *T @ 23:6 is q", "; var q *T @ 23:11 is q", "; address of field A int @ 23:8 is t5", "; field A int @ 23:8 is t3", "; address of field R rune @ 23:13 is t6", "; field R rune @ 23:13 is t4"]),
        ("mapAssign", &["; var m map[int]int @ 27:23 is m", "; var q *T @ 27:6 is q", "; var q *T @ 27:11 is q", "; address of field A int @ 27:8 is t5", "; field A int @ 27:8 is t3", "; address of field B int @ 27:13 is t6", "; field B int @ 27:13 is t4"]),
        ("keyOnly", &["; var s []int @ 31:18 is s", "; var q *T @ 31:6 is q", "; address of field A int @ 31:8 is t4", "; field A int @ 31:8 is t2"]),
        ("valueOnly", &["; var s []int @ 35:21 is s", "; var q *T @ 35:9 is q", "; address of field B int @ 35:11 is t6", "; field B int @ 35:11 is t5"]),
        ("define", &["; var n int @ 39:2 is 0", "; var s []int @ 40:20 is s", "; var i int @ 40:6 is t3", "; var x int @ 40:9 is t6", "; var i int @ 41:8 is t3", "; var x int @ 41:12 is t6", "; *ast.BinaryExpr @ 41:8 is t7", "; var n int @ 41:3 is t8", "; var n int @ 43:9 is t1"]),
        ("defineEscape", &["; var s []int @ 46:20 is s", "; var i int @ 46:6 is t2", "; var x int @ 46:9 is t5", "; *ast.FuncLit @ 47:10 is t8", "; var sink func() @ 47:3 is t8"]),
        ("defineMapEscape", &["; var m map[string]int @ 51:20 is m", "; var k string @ 51:6 is t3", "; var v int @ 51:9 is t4", "; *ast.FuncLit @ 52:10 is t7", "; var sink func() @ 52:3 is t7"]),
        ("idxAssign", &["; var s []int @ 67:25 is s", "; var a []int @ 67:6 is a", "; var a []int @ 67:12 is a", "; *ast.IndexExpr @ 67:6 is t2", "; *ast.IndexExpr @ 67:12 is t5"]),
    ];
    let mut bad = Vec::new();
    for (fname, want) in cases {
        let asm = build(SRC, fname, BuilderMode::GLOBAL_DEBUG);
        let got: Vec<&str> =
            asm.lines().map(str::trim).filter(|l| l.starts_with("; ") && !l.starts_with("; func ")).collect();
        if got != *want {
            bad.push(format!("{fname}:\nwant {want:#?}\n{asm}"));
        }
    }
    assert!(bad.is_empty(), "{} of {} differ:\n{}", bad.len(), cases.len(), bad.join("\n"));
}

/// Range-over-func: the synthetic yield function (`rfAssign$1`) also
/// evaluates both lvalues before storing either (go/ssa `rangeFunc`), so
/// `m[q.B]` reads `q.B` before `q.A` is stored. ssadump v0.52, normalized as
/// above, up to the end of the yield-loop block (the yield-invalid block's
/// panic is spelled differently: guff panics with the string constant).
#[test]
fn test_range_func_yield_assign_v052() {
    let want_instrs: &[&str] = &["t0 = *jump$1", "t1 = t0 == 0", "if t1 goto 1 else 2", "*jump$1 = -1", "t2 = *q", "t3 = *m", "t4 = *q", "t5 = &t4.B [#1]", "t6 = *t5", "t7 = &t2.A [#0]", "*t7 = arg0", "t3[t6] = arg1", "*jump$1 = 0", "return true"];
    let asm = build_nth(SRC, "rfAssign", BuilderMode::default(), Some(0));
    let got = instrs(&asm);
    assert_eq!(&got[..want_instrs.len().min(got.len())], want_instrs, "{asm}");
    let want_refs: &[&str] = &["; var q *T @ 72:6 is t2", "; var m map[int]int @ 72:11 is t3", "; var q *T @ 72:13 is t4", "; address of field B int @ 72:15 is t5", "; *ast.SelectorExpr @ 72:13 is t6", "; address of field A int @ 72:8 is t7", "; field A int @ 72:8 is arg0"];
    let asm = build_nth(SRC, "rfAssign", BuilderMode::GLOBAL_DEBUG, Some(0));
    let got: Vec<&str> =
        asm.lines().map(str::trim).filter(|l| l.starts_with("; ") && !l.starts_with("; func ")).collect();
    assert_eq!(got, want_refs, "{asm}");
}
