//! Field selections in go/ssa (x/tools v0.52) and honnef's go/ir v0.8.2:
//! `emitFieldSelection` ends with `emitDebugRef(f, e.Sel, v, wantAddr)`, so
//! every `x.f` records the field's address (or, for a struct value, its
//! `Field` value) against the field Ident — once more for `&x.f` and before
//! the stored value for `x.f = v`, because `lazyAddress.address`/`store` add
//! their own ref to the same Ident.

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

const SRC: &str = r#"package p

type In struct{ X, Y int }

type T struct {
	A, B int
	In
	P  *In
	Arr [2]int
}

type U struct{ *T }

func f() int { return 1 }

func readVal(t T) int      { return t.A }
func readPtr(p *T) int     { return p.A }
func readLocal() int {
	var t T
	return t.B
}
func readPromoted(t T) int    { return t.X }
func readPromotedPtr(p *T) int { return p.Y }
func readIndirectEmbed(u U) int { return u.X }
func readThroughPtrField(t T) int { return t.P.X }
func readCall() int          { return mk().A }
func readParen(p *T) int     { return (p).B }
func mk() T                  { return T{} }

func storeVal(t T) T {
	t.A = f()
	return t
}
func storePtr(p *T) { p.B = f() }
func storePromoted(p *T) { p.X = f() }
func storeIndirectEmbed(u U) { u.Y = f() }
func addrOf(p *T) *int { return &p.A }
func addrOfLocal() *int {
	var t T
	return &t.B
}
func opAssign(p *T) { p.A += f() }
func incDec(p *T) { p.B++ }
func arrElem(p *T) int { return p.Arr[1] }
func arrStore(p *T) { p.Arr[0] = f() }
func rangeKV(p *T, s []int) {
	for p.A, p.B = range s {
	}
}
func multi(p *T) { p.A, p.B = p.B, p.A }
func blankField(p *T) { _ = p.A }

func methodCall(p *T) { p.P.m() }
func (*In) m() {}
func g(b bool) T { return T{} }
func condCall(a, b bool) int  { return g(a && b).A }
func condLit(a, b bool) int   { return T{A: f(), B: 0, Arr: [2]int{}, P: nil, In: In{X: b2i(a || b)}}.X }
func b2i(b bool) int          { if b { return 1 }; return 0 }
"#;

/// Each expectation is the DebugRef lines of `ssadump -build=DF` of
/// golang.org/x/tools v0.52.0 built with go1.27.1, minus the references to
/// called functions, package qualifier dropped.
#[test]
fn test_field_selection_debugrefs_v052() {
    let cases: &[(&str, &[&str])] = &[
        ("readVal", &["; address of var t T @ 16:37 is t0", "; address of field A int @ 16:39 is t1", "; *ast.SelectorExpr @ 16:37 is t2"]),
        ("readPtr", &["; var p *T @ 17:37 is p", "; address of field A int @ 17:39 is t0", "; *ast.SelectorExpr @ 17:37 is t1"]),
        ("readLocal", &["; address of var t T @ 19:6 is t0", "; address of var t T @ 20:9 is t0", "; address of field B int @ 20:11 is t1", "; *ast.SelectorExpr @ 20:9 is t2"]),
        ("readPromoted", &["; address of var t T @ 22:40 is t0", "; address of field X int @ 22:42 is t2", "; *ast.SelectorExpr @ 22:40 is t3"]),
        ("readPromotedPtr", &["; var p *T @ 23:41 is p", "; address of field Y int @ 23:43 is t1", "; *ast.SelectorExpr @ 23:41 is t2"]),
        ("readIndirectEmbed", &["; var u U @ 24:42 is u", "; address of field X int @ 24:44 is t2", "; *ast.SelectorExpr @ 24:42 is t3"]),
        ("readThroughPtrField", &["; address of var t T @ 25:44 is t0", "; address of field P *In @ 25:46 is t1", "; *ast.SelectorExpr @ 25:44 is t2", "; address of field X int @ 25:48 is t3", "; *ast.SelectorExpr @ 25:44 is t4"]),
        ("readCall", &["; *ast.CallExpr @ 26:39 is t0", "; field A int @ 26:44 is t1", "; *ast.SelectorExpr @ 26:39 is t1"]),
        ("readParen", &["; var p *T @ 27:40 is p", "; address of field B int @ 27:43 is t0", "; *ast.SelectorExpr @ 27:39 is t1"]),
        ("storeVal", &["; address of var t T @ 31:2 is t0", "; *ast.CallExpr @ 31:8 is t1", "; address of field A int @ 31:4 is t2", "; field A int @ 31:4 is t1", "; var t T @ 32:9 is t3"]),
        ("storePtr", &["; var p *T @ 34:23 is p", "; *ast.CallExpr @ 34:29 is t0", "; address of field B int @ 34:25 is t1", "; field B int @ 34:25 is t0"]),
        ("storePromoted", &["; var p *T @ 35:28 is p", "; *ast.CallExpr @ 35:34 is t1", "; address of field X int @ 35:30 is t2", "; field X int @ 35:30 is t1"]),
        ("storeIndirectEmbed", &["; var u U @ 36:32 is u", "; *ast.CallExpr @ 36:38 is t2", "; address of field Y int @ 36:34 is t3", "; field Y int @ 36:34 is t2"]),
        ("addrOf", &["; var p *T @ 37:34 is p", "; address of field A int @ 37:36 is t0", "; address of field A int @ 37:36 is t0", "; *ast.UnaryExpr @ 37:33 is t0"]),
        ("addrOfLocal", &["; address of var t T @ 39:6 is t0", "; address of var t T @ 40:10 is t0", "; address of field B int @ 40:12 is t1", "; address of field B int @ 40:12 is t1", "; *ast.UnaryExpr @ 40:9 is t1"]),
        ("opAssign", &["; var p *T @ 42:23 is p", "; *ast.CallExpr @ 42:30 is t0", "; address of field A int @ 42:25 is t1", "; address of field A int @ 42:25 is t4", "; field A int @ 42:25 is t3"]),
        ("incDec", &["; var p *T @ 43:21 is p", "; address of field B int @ 43:23 is t0", "; address of field B int @ 43:23 is t3", "; field B int @ 43:23 is t2"]),
        ("arrElem", &["; var p *T @ 44:33 is p", "; address of field Arr [2]int @ 44:35 is t0", "; address of field Arr [2]int @ 44:35 is t0", "; *ast.IndexExpr @ 44:33 is t2"]),
        ("arrStore", &["; var p *T @ 45:23 is p", "; address of field Arr [2]int @ 45:25 is t0", "; address of field Arr [2]int @ 45:25 is t0", "; *ast.CallExpr @ 45:34 is t1", "; *ast.IndexExpr @ 45:23 is t1"]),
        ("rangeKV", &["; var s []int @ 47:23 is s", "; var p *T @ 47:6 is p", "; var p *T @ 47:11 is p", "; address of field A int @ 47:8 is t6", "; field A int @ 47:8 is t2", "; address of field B int @ 47:13 is t7", "; field B int @ 47:13 is t5"]),
        ("multi", &["; var p *T @ 50:20 is p", "; var p *T @ 50:25 is p", "; var p *T @ 50:31 is p", "; address of field B int @ 50:33 is t0", "; *ast.SelectorExpr @ 50:31 is t1", "; var p *T @ 50:36 is p", "; address of field A int @ 50:38 is t2", "; *ast.SelectorExpr @ 50:36 is t3", "; address of field A int @ 50:22 is t4", "; field A int @ 50:22 is t1", "; address of field B int @ 50:27 is t5", "; field B int @ 50:27 is t3"]),
        ("blankField", &["; var p *T @ 51:29 is p", "; address of field A int @ 51:31 is t0", "; *ast.SelectorExpr @ 51:29 is t1"]),
        ("methodCall", &["; var p *T @ 53:25 is p", "; address of field P *In @ 53:27 is t0", "; *ast.SelectorExpr @ 53:25 is t1", "; *ast.CallExpr @ 53:25 is t2"]),
        ("condCall", &["; var a bool @ 56:42 is a", "; var b bool @ 56:47 is b", "; *ast.BinaryExpr @ 56:42 is t0", "; *ast.CallExpr @ 56:40 is t1", "; field A int @ 56:50 is t2", "; *ast.SelectorExpr @ 56:40 is t2"]),
        ("condLit", &["; *ast.CallExpr @ 57:45 is t2", "; *ast.CallExpr @ 57:45 is t2", "; *ast.BasicLit @ 57:53 is 0", "; *ast.CompositeLit @ 57:61 is [2]int{}", "; *ast.CompositeLit @ 57:61 is [2]int{}", "; var a bool @ 57:93 is a", "; var b bool @ 57:98 is b", "; *ast.BinaryExpr @ 57:93 is t9", "; *ast.CallExpr @ 57:89 is t10", "; *ast.CallExpr @ 57:89 is t10", "; *ast.CompositeLit @ 57:83 is t11", "; *ast.CompositeLit @ 57:83 is t11", "; *ast.CompositeLit @ 57:40 is t12", "; field X int @ 57:103 is t14", "; *ast.SelectorExpr @ 57:40 is t14"]),

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

/// Without debug info (`BuilderMode::default`) no field ref is emitted.
#[test]
fn test_field_selection_no_debugrefs_without_debug_info() {
    for fname in ["readPtr", "storePtr", "addrOf", "opAssign"] {
        let asm = build(SRC, fname, BuilderMode::default());
        assert!(!asm.contains("field "), "{fname}:\n{asm}");
    }
}
