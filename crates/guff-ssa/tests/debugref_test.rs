//! DebugRef + debug-mode emission tests (Milestone D, chunk D05).
//!
//! Verifies that DebugRef pseudo-instructions are emitted only when the
//! declaring package has debug info enabled (via the `GLOBAL_DEBUG` builder
//! mode), that they carry the source object / expression description, and that
//! the disassembler renders them in go/ssa's `; descr @ line:col is name` form.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff::ast::Decl;
use guff_types::{Checker, Config};
use guff_ssa::builder::Builder;
use guff_ssa::program::Program;
use guff_ssa::mode::BuilderMode;
use guff_ssa::instr::InstrData;
use guff_ssa::ids::FuncId;

const SRC: &str = "package p\nfunc f(x int) int {\n\tx = x + 1\n\treturn x\n}";

/// Builds `SRC`'s `f` under `mode` and returns the finished program + func id.
/// Positions are inspected before/after `finish_function` as noted per test.
fn build(mode: BuilderMode) -> (Program, FuncId, std::sync::Arc<FileSet>) {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", SRC.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);

    let mut prog = Program::new(
        mode,
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    prog.set_fset(fset.clone());

    let type_pkg_id = check.pkg;
    let ssa_pkg_id = guff_ssa::create::create_package(&mut prog, type_pkg_id);

    let fd = file.decls.iter().find_map(|d| match d {
        Decl::FuncDecl(fd) => Some(fd),
        _ => None,
    }).expect("no FuncDecl");

    let fn_id = guff_ssa::create::create_function(&mut prog, fd.name.name.clone(), None, Some(ssa_pkg_id));

    // Register parameters from the signature (create::create_params).
    let obj_f = prog.info.defs.get(&fd.name.id).unwrap().unwrap();
    let sig_id = obj_f.typ(&prog.object_arena).unwrap();
    prog.functions.get_mut(fn_id).signature = Some(sig_id);
    guff_ssa::create::create_params(&mut prog, fn_id);

    let mut builder = Builder::new(&mut prog, fn_id);
    let entry = builder.new_basic_block("entry".to_string());
    builder.set_block(Some(entry));
    builder.stmt(&guff::ast::Stmt::BlockStmt(fd.body.clone().unwrap()));
    drop(builder);

    (prog, fn_id, fset)
}

#[test]
fn test_debugrefs_emitted_in_debug_mode() {
    let (prog, fn_id, fset) = build(BuilderMode::GLOBAL_DEBUG);
    let f = prog.functions.get(fn_id);

    // Collect DebugRefs. Expect: `x` (RHS var read), BinaryExpr `x + 1`,
    // store-DebugRef for LHS `x`, and `x` again in the return.
    let mut var_lines = Vec::new();
    let mut binop_seen = false;
    for (id, data) in f.instrs.iter() {
        if let InstrData::DebugRef(d) = data {
            assert!(!d.is_addr, "value DebugRefs here should not be address refs");
            let pos = f.pos(id);
            assert!(pos.is_valid(), "DebugRef should carry a valid position");
            let line = fset.position(pos).line;
            match d.object {
                Some(_) => var_lines.push(line), // ident `x`
                None => {
                    assert_eq!(d.expr_descr, "*ast.BinaryExpr");
                    binop_seen = true;
                }
            }
        }
    }

    assert!(binop_seen, "expected a DebugRef for the `x + 1` BinaryExpr");
    var_lines.sort();
    assert_eq!(
        var_lines,
        vec![3, 3, 4],
        "expected var-`x` DebugRefs: RHS read + store on line 3, return on line 4"
    );
}

#[test]
fn test_no_debugrefs_without_debug_mode() {
    let (prog, fn_id, _) = build(BuilderMode::default());
    let f = prog.functions.get(fn_id);
    let count = f.instrs.iter().filter(|(_, d)| matches!(d, InstrData::DebugRef(_))).count();
    assert_eq!(count, 0, "no DebugRefs should be emitted when debug info is disabled");
}

#[test]
fn test_debugref_disassembly() {
    let (mut prog, fn_id, _) = build(BuilderMode::GLOBAL_DEBUG);
    // Assign register numbers so value refs print as tN; this does not run the
    // lift pass (which would require NAIVE_FORM off + allocs), so DebugRefs are
    // left intact for inspection.
    prog.functions.get_mut(fn_id).number_registers();

    let f = prog.functions.get(fn_id);
    let asm = guff_ssa::print::disassemble_function(f, &prog);

    // The RHS `x` read (line 3, col 6) and the return `x` (line 4, col 9). Each
    // resolves to the *loaded* value of the spilled `x` cell (go/ssa attaches
    // the DebugRef to the load register, e.g. `t4`/`t6`), which here print as
    // `t0` and `t2`.
    assert!(
        asm.contains("; var x int @ 3:6 is t0"),
        "missing RHS var DebugRef line; got:\n{asm}"
    );
    assert!(
        asm.contains("; var x int @ 4:9 is t2"),
        "missing return var DebugRef line; got:\n{asm}"
    );
    // The `x + 1` BinaryExpr DebugRef (non-ident: AST node name description).
    assert!(
        asm.contains("*ast.BinaryExpr @ 3:6 is "),
        "missing BinaryExpr DebugRef line; got:\n{asm}"
    );
}

/// Builds `f` from `src` under GLOBAL_DEBUG and returns its disassembly.
fn disasm_debug(src: &str) -> String {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse failed");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    let mut prog = Program::new(
        BuilderMode::GLOBAL_DEBUG,
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    prog.set_fset(fset.clone());
    let ssa_pkg_id = guff_ssa::create::create_package(&mut prog, check.pkg);
    let fd = file
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::FuncDecl(fd) if fd.name.name == "f" => Some(fd),
            _ => None,
        })
        .expect("no func f");
    let fn_id = guff_ssa::create::create_function(&mut prog, fd.name.name.clone(), None, Some(ssa_pkg_id));
    let obj_f = prog.info.defs.get(&fd.name.id).unwrap().unwrap();
    let sig_id = obj_f.typ(&prog.object_arena).unwrap();
    prog.functions.get_mut(fn_id).signature = Some(sig_id);
    guff_ssa::create::create_params(&mut prog, fn_id);
    let mut builder = Builder::new(&mut prog, fn_id);
    let entry = builder.new_basic_block("entry".to_string());
    builder.set_block(Some(entry));
    builder.stmt(&guff::ast::Stmt::BlockStmt(fd.body.clone().unwrap()));
    drop(builder);
    prog.functions.get_mut(fn_id).number_registers();
    guff_ssa::print::disassemble_function(prog.functions.get(fn_id), &prog)
}

/// Every lvalue shape that carries its source expression for DebugRef
/// (`address.expr` / `lazyAddress.expr` in go/ssa): an identifier (also
/// parenthesized), `*p`, a field `s.f` (go/ssa records the field Ident, not
/// the selector), an array element of a variable, a slice element, and a
/// composite literal whose address is taken. Plus the blank identifier, which
/// must record nothing.
///
/// This pins guff's *current* output (written on main before lvalues stopped
/// cloning their syntax, and unchanged by it), in guff's printer format. Known
/// differences from go/ssa, all pre-existing:
/// - go/ssa also emits `*ast.BasicLit @ 13:13` for the `f: 7` element slot of
///   `&S{f: 7}` (`compLit` builds `&address{…, expr: e}`); guff's composite-
///   literal element stores (`builder/stmt.rs`) pass `expr: None`.
/// - go/types prints a field object as `field f int`, and constants as `1:int`.
#[test]
fn test_debugrefs_for_every_lvalue_shape() {
    const SRC: &str = "package p
type S struct{ f int }
func f(p *int, sl []int) {
	var a [2]int
	var s S
	x := 0
	x = 1
	(x) = 2
	*p = 3
	s.f = 4
	a[1] = 5
	sl[0] = 6
	q := &S{f: 7}
	_ = q
	_ = x
	_, _ = a, s
}";
    let asm = disasm_debug(SRC);
    let refs: Vec<&str> = asm
        .lines()
        .filter_map(|l| l.split_once("; ").map(|(_, r)| r))
        .filter(|r| r.contains(" @ "))
        .collect();
    // The whole list, in emission order. The lvalue-shape lines are the
    // stores through each location; the rest are reads and `&` results.
    let want = [
        "address of var a [2]int @ 4:6 is t0",
        "address of var s S @ 5:6 is t1",
        "var x int @ 6:2 is 0",
        "var x int @ 7:2 is 1",             // x = 1
        "var x int @ 8:3 is 2",             // (x) = 2: parens are stripped
        "var p *int @ 9:3 is t3",
        "*ast.StarExpr @ 9:2 is 3",         // *p = 3
        "address of var s S @ 10:2 is t1",
        "var f int @ 10:4 is 4",            // s.f = 4: the field Ident
        "address of var a [2]int @ 11:2 is t0",
        "*ast.IndexExpr @ 11:2 is 5",       // a[1] = 5
        "var sl []int @ 12:2 is t6",
        "*ast.IndexExpr @ 12:2 is 6",       // sl[0] = 6
        "address of *ast.CompositeLit @ 13:8 is t9", // &S{f: 7}
        "*ast.UnaryExpr @ 13:7 is t9",
        "var q *S @ 13:2 is t9",
        "var q *S @ 14:6 is t11",
        "var x int @ 15:6 is t12",
        "var a [2]int @ 16:9 is t13",
        "var s S @ 16:12 is t14",
    ];
    // `_ = …` and `_, _ = …` record nothing for the blank identifiers.
    assert_eq!(refs, want, "DebugRefs; full disassembly:\n{asm}");
}
