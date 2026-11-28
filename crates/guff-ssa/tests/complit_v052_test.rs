//! Composite literals in x/tools v0.52's go/ssa: `compLit` stores each
//! field/element as soon as its value is evaluated (v0.50 buffered every
//! store until all elements had been evaluated). honnef's IR, which buildir
//! builds for staticcheck, keeps the buffered order (`IR_COMPLIT_STOREBUF`).

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
        .filter(|l| l.starts_with('\t'))
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
}

type In struct{ X, Y int }

type Outer struct {
	In
	Z int
}

type N struct {
	A T
	P *T
	C int
}

func f() int { return 1 }
func g() int { return 2 }
func h() *T  { return nil }

func structKeyed() T      { return T{A: f(), B: g()} }
func structPos() T        { return T{f(), g()} }
func structPartial() T    { return T{B: g()} }
func structEmpty() T      { return T{} }
func structPromoted() Outer { return Outer{X: f(), Z: g()} }
func structNested() N     { return N{A: T{A: f()}, P: &T{B: g()}, C: f()} }
func structImplicitPtr() []*T { return []*T{{A: f()}, {B: g()}} }

func arrayFull() [3]int    { return [3]int{f(), g(), f()} }
func arrayPartial() [3]int { return [3]int{f()} }
func arrayEllipsis() [3]int { return [...]int{f(), g(), f()} }
func arrayKeyed() [4]int   { return [4]int{1: f(), 3: g()} }
func arrayMixed() [5]int   { return [5]int{f(), 3: g(), f()} }

func sliceLit() []int      { return []int{f(), g()} }
func sliceKeyed() []int    { return []int{2: f()} }
func sliceEmpty() []int    { return []int{} }
func sliceNested() [][]int { return [][]int{{f()}, {g()}} }

func mapLit() map[string]int { return map[string]int{"a": f(), "b": g()} }
func mapPtrKey() map[*T]int  { return map[*T]int{{A: f()}: g()} }
func mapPtrVal() map[int]*T  { return map[int]*T{f(): {A: g()}} }
func mapEmpty() map[int]int  { return map[int]int{} }

func selfRef(x T) T {
	x = T{A: x.B, B: x.A}
	return x
}

func define() int {
	x := T{A: f(), B: g()}
	return x.A
}

func varDecl() T {
	var x = T{A: f(), B: g()}
	return x
}

func addrOf() *T { return &T{A: f(), B: g()} }

func iface() any { return T{A: f(), B: g()} }

func call(t T) {}

func arg() { call(T{A: f(), B: g()}) }

func escapes() func() T {
	x := T{A: f(), B: g()}
	return func() T { return x }
}

func ptrDeref(p *T) {
	*p = T{A: p.B, B: f()}
}

func arrayAssign(a [3]int) [3]int {
	a = [3]int{a[2], f(), a[0]}
	return a
}

func conv() [2]any { return [2]any{f(), "s"} }

func selfArray(ar [3]int) [3]int {
	ar = [3]int{ar[2]}
	return ar
}
"#;

/// Each expectation is `ssadump -build=F` of golang.org/x/tools v0.52.0
/// (built with go1.27.1) on `SRC`, with the type column and the `:type`
/// suffix of constants (which guff's printer does not write) dropped.
#[test]
fn test_complit_stores_interleave_with_elements_v052() {
    let cases: &[(&str, &[&str])] = &[
        ("structKeyed", &["t0 = local T (complit)", "t1 = &t0.A [#0]", "t2 = f()", "*t1 = t2", "t3 = &t0.B [#1]", "t4 = g()", "*t3 = t4", "t5 = *t0", "return t5"]),
        ("structPos", &["t0 = local T (complit)", "t1 = &t0.A [#0]", "t2 = f()", "*t1 = t2", "t3 = &t0.B [#1]", "t4 = g()", "*t3 = t4", "t5 = *t0", "return t5"]),
        ("structPartial", &["t0 = local T (complit)", "t1 = &t0.B [#1]", "t2 = g()", "*t1 = t2", "t3 = *t0", "return t3"]),
        ("structEmpty", &["return T{}"]),
        ("structPromoted", &["t0 = local Outer (complit)", "t1 = &t0.In [#0]", "t2 = &t1.X [#0]", "t3 = f()", "*t2 = t3", "t4 = &t0.Z [#1]", "t5 = g()", "*t4 = t5", "t6 = *t0", "return t6"]),
        ("structNested", &["t0 = local N (complit)", "t1 = &t0.A [#0]", "t2 = local T (complit)", "t3 = &t2.A [#0]", "t4 = f()", "*t3 = t4", "t5 = *t2", "*t1 = t5", "t6 = &t0.P [#1]", "t7 = new T (complit)", "t8 = &t7.B [#1]", "t9 = g()", "*t8 = t9", "*t6 = t7", "t10 = &t0.C [#2]", "t11 = f()", "*t10 = t11", "t12 = *t0", "return t12"]),
        ("structImplicitPtr", &["t0 = new [2]*T (slicelit)", "t1 = &t0[0]", "t2 = new T (complit)", "t3 = &t2.A [#0]", "t4 = f()", "*t3 = t4", "*t1 = t2", "t5 = &t0[1]", "t6 = new T (complit)", "t7 = &t6.B [#1]", "t8 = g()", "*t7 = t8", "*t5 = t6", "t9 = slice t0[:]", "return t9"]),
        ("arrayFull", &["t0 = local [3]int (complit)", "t1 = &t0[0]", "t2 = f()", "*t1 = t2", "t3 = &t0[1]", "t4 = g()", "*t3 = t4", "t5 = &t0[2]", "t6 = f()", "*t5 = t6", "t7 = *t0", "return t7"]),
        ("arrayPartial", &["t0 = local [3]int (complit)", "t1 = &t0[0]", "t2 = f()", "*t1 = t2", "t3 = *t0", "return t3"]),
        ("arrayEllipsis", &["t0 = local [3]int (complit)", "t1 = &t0[0]", "t2 = f()", "*t1 = t2", "t3 = &t0[1]", "t4 = g()", "*t3 = t4", "t5 = &t0[2]", "t6 = f()", "*t5 = t6", "t7 = *t0", "return t7"]),
        ("arrayKeyed", &["t0 = local [4]int (complit)", "t1 = &t0[1]", "t2 = f()", "*t1 = t2", "t3 = &t0[3]", "t4 = g()", "*t3 = t4", "t5 = *t0", "return t5"]),
        ("arrayMixed", &["t0 = local [5]int (complit)", "t1 = &t0[0]", "t2 = f()", "*t1 = t2", "t3 = &t0[3]", "t4 = g()", "*t3 = t4", "t5 = &t0[4]", "t6 = f()", "*t5 = t6", "t7 = *t0", "return t7"]),
        ("sliceLit", &["t0 = new [2]int (slicelit)", "t1 = &t0[0]", "t2 = f()", "*t1 = t2", "t3 = &t0[1]", "t4 = g()", "*t3 = t4", "t5 = slice t0[:]", "return t5"]),
        ("sliceKeyed", &["t0 = new [3]int (slicelit)", "t1 = &t0[2]", "t2 = f()", "*t1 = t2", "t3 = slice t0[:]", "return t3"]),
        ("sliceEmpty", &["t0 = new [0]int (slicelit)", "t1 = slice t0[:]", "return t1"]),
        ("sliceNested", &["t0 = new [2][]int (slicelit)", "t1 = &t0[0]", "t2 = new [1]int (slicelit)", "t3 = &t2[0]", "t4 = f()", "*t3 = t4", "t5 = slice t2[:]", "*t1 = t5", "t6 = &t0[1]", "t7 = new [1]int (slicelit)", "t8 = &t7[0]", "t9 = g()", "*t8 = t9", "t10 = slice t7[:]", "*t6 = t10", "t11 = slice t0[:]", "return t11"]),
        ("mapLit", &["t0 = make map[string]int 2", "t1 = f()", "t0[\"a\"] = t1", "t2 = g()", "t0[\"b\"] = t2", "return t0"]),
        ("mapPtrKey", &["t0 = make map[*T]int 1", "t1 = new T (complit)", "t2 = &t1.A [#0]", "t3 = f()", "*t2 = t3", "t4 = g()", "t0[t1] = t4", "return t0"]),
        ("mapPtrVal", &["t0 = make map[int]*T 1", "t1 = f()", "t2 = new T (complit)", "t3 = &t2.A [#0]", "t4 = g()", "*t3 = t4", "t0[t1] = t2", "return t0"]),
        ("mapEmpty", &["t0 = make map[int]int 0", "return t0"]),
        ("selfRef", &["t0 = local T (x)", "*t0 = x", "t1 = local T (complit)", "t2 = &t1.A [#0]", "t3 = &t0.B [#1]", "t4 = *t3", "*t2 = t4", "t5 = &t1.B [#1]", "t6 = &t0.A [#0]", "t7 = *t6", "*t5 = t7", "t8 = *t1", "*t0 = t8", "t9 = *t0", "return t9"]),
        ("define", &["t0 = local T (x)", "t1 = local T (complit)", "t2 = &t1.A [#0]", "t3 = f()", "*t2 = t3", "t4 = &t1.B [#1]", "t5 = g()", "*t4 = t5", "t6 = *t1", "*t0 = t6", "t7 = &t0.A [#0]", "t8 = *t7", "return t8"]),
        ("varDecl", &["t0 = local T (complit)", "t1 = &t0.A [#0]", "t2 = f()", "*t1 = t2", "t3 = &t0.B [#1]", "t4 = g()", "*t3 = t4", "t5 = *t0", "return t5"]),
        ("addrOf", &["t0 = new T (complit)", "t1 = &t0.A [#0]", "t2 = f()", "*t1 = t2", "t3 = &t0.B [#1]", "t4 = g()", "*t3 = t4", "return t0"]),
        ("iface", &["t0 = local T (complit)", "t1 = &t0.A [#0]", "t2 = f()", "*t1 = t2", "t3 = &t0.B [#1]", "t4 = g()", "*t3 = t4", "t5 = *t0", "t6 = make any <- T (t5)", "return t6"]),
        ("arg", &["t0 = local T (complit)", "t1 = &t0.A [#0]", "t2 = f()", "*t1 = t2", "t3 = &t0.B [#1]", "t4 = g()", "*t3 = t4", "t5 = *t0", "t6 = call(t5)", "return"]),
        ("escapes", &["t0 = new T (x)", "t1 = local T (complit)", "t2 = &t1.A [#0]", "t3 = f()", "*t2 = t3", "t4 = &t1.B [#1]", "t5 = g()", "*t4 = t5", "t6 = *t1", "*t0 = t6", "t7 = make closure escapes$1 [t0]", "return t7"]),
        ("ptrDeref", &["t0 = local T (complit)", "t1 = &t0.A [#0]", "t2 = &p.B [#1]", "t3 = *t2", "*t1 = t3", "t4 = &t0.B [#1]", "t5 = f()", "*t4 = t5", "t6 = *t0", "*p = t6", "return"]),
        ("arrayAssign", &["t0 = local [3]int (a)", "*t0 = a", "t1 = local [3]int (complit)", "t2 = &t1[0]", "t3 = &t0[2]", "t4 = *t3", "*t2 = t4", "t5 = &t1[1]", "t6 = f()", "*t5 = t6", "t7 = &t1[2]", "t8 = &t0[0]", "t9 = *t8", "*t7 = t9", "t10 = *t1", "*t0 = t10", "t11 = *t0", "return t11"]),
        ("conv", &["t0 = local [2]any (complit)", "t1 = &t0[0]", "t2 = f()", "t3 = make any <- int (t2)", "*t1 = t3", "t4 = &t0[1]", "t5 = make any <- string (\"s\")", "*t4 = t5", "t6 = *t0", "return t6"]),
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

/// buildir (`IR_COMPLIT_STOREBUF`) keeps the buffered order: every element is
/// evaluated before the first store, as x/tools v0.50 emitted and as honnef's
/// IR orders a struct literal (one `CompositeValue`, stored once). A slice's
/// backing-array stores were never buffered.
#[test]
fn test_complit_stores_stay_buffered_for_buildir() {
    let mode = BuilderMode::IR_COMPLIT_STOREBUF;
    let cases: &[(&str, &[&str])] = &[
        (
            "structKeyed",
            &["t0 = local T (complit)", "t1 = &t0.A [#0]", "t2 = f()", "t3 = &t0.B [#1]", "t4 = g()", "*t1 = t2", "*t3 = t4", "t5 = *t0", "return t5"],
        ),
        (
            "arrayFull",
            &[
                "t0 = local [3]int (complit)",
                "t1 = &t0[0]",
                "t2 = f()",
                "t3 = &t0[1]",
                "t4 = g()",
                "t5 = &t0[2]",
                "t6 = f()",
                "*t1 = t2",
                "*t3 = t4",
                "*t5 = t6",
                "t7 = *t0",
                "return t7",
            ],
        ),
        (
            "sliceLit",
            &["t0 = new [2]int (slicelit)", "t1 = &t0[0]", "t2 = f()", "*t1 = t2", "t3 = &t0[1]", "t4 = g()", "*t3 = t4", "t5 = slice t0[:]", "return t5"],
        ),
    ];
    for (fname, want) in cases {
        let asm = build(SRC, fname, mode);
        assert_eq!(instrs(&asm), *want, "{fname}:\n{asm}");
    }
}

/// The DebugRefs (`-build=DF`, the `; ... is ...` lines other than the
/// references to the called functions, package qualifier dropped; go/ssa's
/// `address of field X` refs for a selector are dropped as well — guff does
/// not record those yet, see PHASE9-LEDGER xtools-substrate #1) of the same functions: each slot's store records the value
/// right after it is evaluated, and an implied `&T{...}` (`[]*T{{...}}`,
/// `map[*T]V{{...}: ...}`, `map[K]*T{k: {...}}`) records its pointer value
/// once more, because v0.52 builds it in `expr` rather than in `assign`.
#[test]
fn test_complit_debugrefs_v052() {
    let cases: &[(&str, &[&str])] = &[
        ("structKeyed", &["; *ast.CallExpr @ 24:41 is t2", "; *ast.CallExpr @ 24:41 is t2", "; *ast.CallExpr @ 24:49 is t4", "; *ast.CallExpr @ 24:49 is t4", "; *ast.CompositeLit @ 24:36 is t5"]),
        ("structPos", &["; *ast.CallExpr @ 25:38 is t2", "; *ast.CallExpr @ 25:38 is t2", "; *ast.CallExpr @ 25:43 is t4", "; *ast.CallExpr @ 25:43 is t4", "; *ast.CompositeLit @ 25:36 is t5"]),
        ("structPartial", &["; *ast.CallExpr @ 26:41 is t2", "; *ast.CallExpr @ 26:41 is t2", "; *ast.CompositeLit @ 26:36 is t3"]),
        ("structEmpty", &["; *ast.CompositeLit @ 27:36 is T{}"]),
        ("structPromoted", &["; *ast.CallExpr @ 28:47 is t3", "; *ast.CallExpr @ 28:47 is t3", "; *ast.CallExpr @ 28:55 is t5", "; *ast.CallExpr @ 28:55 is t5", "; *ast.CompositeLit @ 28:38 is t6"]),
        ("structNested", &["; *ast.CallExpr @ 29:46 is t4", "; *ast.CallExpr @ 29:46 is t4", "; *ast.CompositeLit @ 29:41 is t5", "; *ast.CompositeLit @ 29:41 is t5", "; *ast.CallExpr @ 29:61 is t9", "; *ast.CallExpr @ 29:61 is t9", "; address of *ast.CompositeLit @ 29:56 is t7", "; *ast.UnaryExpr @ 29:55 is t7", "; *ast.UnaryExpr @ 29:55 is t7", "; *ast.CallExpr @ 29:70 is t11", "; *ast.CallExpr @ 29:70 is t11", "; *ast.CompositeLit @ 29:36 is t12"]),
        ("structImplicitPtr", &["; *ast.CallExpr @ 30:49 is t4", "; *ast.CallExpr @ 30:49 is t4", "; address of *ast.CompositeLit @ 30:45 is t2", "; *ast.CompositeLit @ 30:45 is t2", "; *ast.CompositeLit @ 30:45 is t2", "; *ast.CallExpr @ 30:59 is t8", "; *ast.CallExpr @ 30:59 is t8", "; address of *ast.CompositeLit @ 30:55 is t6", "; *ast.CompositeLit @ 30:55 is t6", "; *ast.CompositeLit @ 30:55 is t6", "; *ast.CompositeLit @ 30:40 is t9", "; *ast.CompositeLit @ 30:40 is t9"]),
        ("arrayFull", &["; *ast.CallExpr @ 32:44 is t2", "; *ast.CallExpr @ 32:44 is t2", "; *ast.CallExpr @ 32:49 is t4", "; *ast.CallExpr @ 32:49 is t4", "; *ast.CallExpr @ 32:54 is t6", "; *ast.CallExpr @ 32:54 is t6", "; *ast.CompositeLit @ 32:37 is t7"]),
        ("arrayPartial", &["; *ast.CallExpr @ 33:44 is t2", "; *ast.CallExpr @ 33:44 is t2", "; *ast.CompositeLit @ 33:37 is t3"]),
        ("arrayEllipsis", &["; *ast.CallExpr @ 34:47 is t2", "; *ast.CallExpr @ 34:47 is t2", "; *ast.CallExpr @ 34:52 is t4", "; *ast.CallExpr @ 34:52 is t4", "; *ast.CallExpr @ 34:57 is t6", "; *ast.CallExpr @ 34:57 is t6", "; *ast.CompositeLit @ 34:38 is t7"]),
        ("arrayKeyed", &["; *ast.CallExpr @ 35:47 is t2", "; *ast.CallExpr @ 35:47 is t2", "; *ast.CallExpr @ 35:55 is t4", "; *ast.CallExpr @ 35:55 is t4", "; *ast.CompositeLit @ 35:37 is t5"]),
        ("arrayMixed", &["; *ast.CallExpr @ 36:44 is t2", "; *ast.CallExpr @ 36:44 is t2", "; *ast.CallExpr @ 36:52 is t4", "; *ast.CallExpr @ 36:52 is t4", "; *ast.CallExpr @ 36:57 is t6", "; *ast.CallExpr @ 36:57 is t6", "; *ast.CompositeLit @ 36:37 is t7"]),
        ("sliceLit", &["; *ast.CallExpr @ 38:43 is t2", "; *ast.CallExpr @ 38:43 is t2", "; *ast.CallExpr @ 38:48 is t4", "; *ast.CallExpr @ 38:48 is t4", "; *ast.CompositeLit @ 38:37 is t5", "; *ast.CompositeLit @ 38:37 is t5"]),
        ("sliceKeyed", &["; *ast.CallExpr @ 39:46 is t2", "; *ast.CallExpr @ 39:46 is t2", "; *ast.CompositeLit @ 39:37 is t3", "; *ast.CompositeLit @ 39:37 is t3"]),
        ("sliceEmpty", &["; *ast.CompositeLit @ 40:37 is t1", "; *ast.CompositeLit @ 40:37 is t1"]),
        ("sliceNested", &["; *ast.CallExpr @ 41:46 is t4", "; *ast.CallExpr @ 41:46 is t4", "; *ast.CompositeLit @ 41:45 is t5", "; *ast.CompositeLit @ 41:45 is t5", "; *ast.CompositeLit @ 41:45 is t5", "; *ast.CallExpr @ 41:53 is t9", "; *ast.CallExpr @ 41:53 is t9", "; *ast.CompositeLit @ 41:52 is t10", "; *ast.CompositeLit @ 41:52 is t10", "; *ast.CompositeLit @ 41:52 is t10", "; *ast.CompositeLit @ 41:37 is t11", "; *ast.CompositeLit @ 41:37 is t11"]),
        ("mapLit", &["; *ast.CallExpr @ 43:59 is t1", "; *ast.CallExpr @ 43:69 is t2", "; *ast.CompositeLit @ 43:39 is t0", "; *ast.CompositeLit @ 43:39 is t0"]),
        ("mapPtrKey", &["; *ast.CallExpr @ 44:54 is t3", "; *ast.CallExpr @ 44:54 is t3", "; address of *ast.CompositeLit @ 44:50 is t1", "; *ast.CompositeLit @ 44:50 is t1", "; *ast.CallExpr @ 44:60 is t4", "; *ast.CompositeLit @ 44:39 is t0", "; *ast.CompositeLit @ 44:39 is t0"]),
        ("mapPtrVal", &["; *ast.CallExpr @ 45:50 is t1", "; *ast.CallExpr @ 45:59 is t4", "; *ast.CallExpr @ 45:59 is t4", "; address of *ast.CompositeLit @ 45:55 is t2", "; *ast.CompositeLit @ 45:55 is t2", "; *ast.CompositeLit @ 45:39 is t0", "; *ast.CompositeLit @ 45:39 is t0"]),
        ("mapEmpty", &["; *ast.CompositeLit @ 46:39 is t0", "; *ast.CompositeLit @ 46:39 is t0"]),
        ("selfRef", &["; address of var x T @ 49:11 is t0", "; *ast.SelectorExpr @ 49:11 is t4", "; *ast.SelectorExpr @ 49:11 is t4", "; address of var x T @ 49:19 is t0", "; *ast.SelectorExpr @ 49:19 is t7", "; *ast.SelectorExpr @ 49:19 is t7", "; *ast.CompositeLit @ 49:6 is t8", "; var x T @ 49:2 is t8", "; var x T @ 50:9 is t9"]),
        ("define", &["; *ast.CallExpr @ 54:12 is t3", "; *ast.CallExpr @ 54:12 is t3", "; *ast.CallExpr @ 54:20 is t5", "; *ast.CallExpr @ 54:20 is t5", "; *ast.CompositeLit @ 54:7 is t6", "; var x T @ 54:2 is t6", "; address of var x T @ 55:9 is t0", "; *ast.SelectorExpr @ 55:9 is t8"]),
        ("varDecl", &["; *ast.CallExpr @ 59:15 is t2", "; *ast.CallExpr @ 59:15 is t2", "; *ast.CallExpr @ 59:23 is t4", "; *ast.CallExpr @ 59:23 is t4", "; *ast.CompositeLit @ 59:10 is t5", "; var x T @ 59:6 is t5", "; var x T @ 60:9 is t5"]),
        ("addrOf", &["; *ast.CallExpr @ 63:33 is t2", "; *ast.CallExpr @ 63:33 is t2", "; *ast.CallExpr @ 63:41 is t4", "; *ast.CallExpr @ 63:41 is t4", "; address of *ast.CompositeLit @ 63:28 is t0", "; *ast.UnaryExpr @ 63:27 is t0"]),
        ("iface", &["; *ast.CallExpr @ 65:32 is t2", "; *ast.CallExpr @ 65:32 is t2", "; *ast.CallExpr @ 65:40 is t4", "; *ast.CallExpr @ 65:40 is t4", "; *ast.CompositeLit @ 65:27 is t5"]),
        ("arg", &["; *ast.CallExpr @ 69:24 is t2", "; *ast.CallExpr @ 69:24 is t2", "; *ast.CallExpr @ 69:32 is t4", "; *ast.CallExpr @ 69:32 is t4", "; *ast.CompositeLit @ 69:19 is t5", "; *ast.CallExpr @ 69:14 is t6"]),
        ("escapes", &["; *ast.CallExpr @ 72:12 is t3", "; *ast.CallExpr @ 72:12 is t3", "; *ast.CallExpr @ 72:20 is t5", "; *ast.CallExpr @ 72:20 is t5", "; *ast.CompositeLit @ 72:7 is t6", "; var x T @ 72:2 is t6", "; *ast.FuncLit @ 73:9 is t7"]),
        ("ptrDeref", &["; var p *T @ 77:3 is p", "; var p *T @ 77:12 is p", "; *ast.SelectorExpr @ 77:12 is t3", "; *ast.SelectorExpr @ 77:12 is t3", "; *ast.CallExpr @ 77:20 is t5", "; *ast.CallExpr @ 77:20 is t5", "; *ast.CompositeLit @ 77:7 is t6", "; *ast.StarExpr @ 77:2 is t6"]),
        ("arrayAssign", &["; address of var a [3]int @ 81:13 is t0", "; *ast.IndexExpr @ 81:13 is t4", "; *ast.IndexExpr @ 81:13 is t4", "; *ast.CallExpr @ 81:19 is t6", "; *ast.CallExpr @ 81:19 is t6", "; address of var a [3]int @ 81:24 is t0", "; *ast.IndexExpr @ 81:24 is t9", "; *ast.IndexExpr @ 81:24 is t9", "; *ast.CompositeLit @ 81:6 is t10", "; var a [3]int @ 81:2 is t10", "; var a [3]int @ 82:9 is t11"]),
        ("conv", &["; *ast.CallExpr @ 85:36 is t2", "; *ast.CallExpr @ 85:36 is t3", "; *ast.BasicLit @ 85:41 is t5", "; *ast.CompositeLit @ 85:29 is t6"]),
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

/// `IR_IN_PLACE_PARTIAL_ARRAYS` fills `ar = [3]int{ar[2]}` into `ar` itself,
/// so it buffers the stores even without `IR_COMPLIT_STOREBUF`: `ar[2]` is
/// read before the memclear and the element store write `ar`.
#[test]
fn test_in_place_partial_array_reads_before_it_writes() {
    let asm = build(SRC, "selfArray", BuilderMode::IR_IN_PLACE_PARTIAL_ARRAYS);
    let want = [
        "t0 = local [3]int (ar)",
        "*t0 = ar",
        "t1 = &t0[0]",
        "t2 = &t0[2]",
        "t3 = *t2",
        "*t0 = [3]int{}",
        "*t1 = t3",
        "t4 = *t0",
        "return t4",
    ];
    assert_eq!(instrs(&asm), want, "{asm}");
}
