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
    disasm_debug_mode(src, BuilderMode::GLOBAL_DEBUG)
}

/// [`disasm_debug`] under an explicit builder mode.
fn disasm_debug_mode(src: &str, mode: BuilderMode) -> String {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse failed");
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
/// - go/types prints constants as `1:int`.
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
        "address of field f int @ 10:4 is t4", // emitFieldSelection
        "field f int @ 10:4 is 4",          // s.f = 4: the field Ident
        "address of var a [2]int @ 11:2 is t0",
        "*ast.IndexExpr @ 11:2 is 5",       // a[1] = 5
        "var sl []int @ 12:2 is t6",
        "*ast.IndexExpr @ 12:2 is 6",       // sl[0] = 6
        "*ast.BasicLit @ 13:13 is 7",       // the `f: 7` element slot
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

/// Composite-literal element slots and the final slice/map store carry their
/// source expression (go/ssa `compLit`: `&address{…, expr: e}` for a struct
/// field, an array/slice element, and the `Slice`/`MakeMap` stored into the
/// literal's location; v0.52 has no memclear, and v0.50 only reached
/// compLit with `isZero`), and every
/// store-DebugRef names the *converted* value (`store.Val`).
///
/// `want` is `ssadump -build=DF` of golang.org/x/tools v0.52.0 on this source
/// with the values dropped (go/ssa prints after lifting; the one difference
/// is the declarations `var a`/`var s`/`var ar`/`var w`, which lifting turns
/// from `address of` into the zero value).
#[test]
fn test_debugrefs_for_composite_literal_slots() {
    const SRC: &str = "package p

type S struct{ f int; g any }
type E struct{ S }

func f(p *int, sl []int, y int, z error) {
	var a [2]int
	var s S
	x := 0
	_ = x
	s = S{f: y, g: y}
	_ = s
	s = S{y, 7}
	a = [2]int{y, 1: y}
	_ = a
	b := [...]any{3: y}
	_ = b
	c := []any{y, z}
	_ = c
	m := map[int]any{1: y}
	_ = m
	q := &S{f: 7}
	_ = q
	t := []S{{f: y}}
	_ = t
	var ar [3]int
	ar = [3]int{y}
	_ = ar
	var w any
	w = y
	_ = w
	n := []*S{{f: y}}
	_ = n
}";
    let asm = disasm_debug(SRC);
    let refs: Vec<(&str, &str)> = asm
        .lines()
        .filter_map(|l| l.split_once("; ").map(|(_, r)| r))
        .filter_map(|r| r.split_once(" is "))
        .filter(|(d, _)| d.contains(" @ "))
        .collect();
    let descrs: Vec<String> = refs
        .iter()
        .map(|(d, _)| d.strip_prefix("address of ").filter(|d| d.starts_with("var ")).unwrap_or(d).to_string())
        .collect();
    let want = [
        "var a [2]int @ 7:6", "var s S @ 8:6", "var x int @ 9:2", "var x int @ 10:6",
        // s = S{f: y, g: y}: each value read, then its slot store (v0.52
        // interleaves them; v0.50 read both values before either store)
        "var y int @ 11:11", "var y int @ 11:11", "var y int @ 11:17", "var y int @ 11:17",
        "*ast.CompositeLit @ 11:6", "var s S @ 11:2", "var s S @ 12:6",
        // s = S{y, 7}: the constant has no read ref, only the slot's
        "var y int @ 13:8", "var y int @ 13:8", "*ast.BasicLit @ 13:11",
        "*ast.CompositeLit @ 13:6", "var s S @ 13:2",
        "var y int @ 14:13", "var y int @ 14:13", "var y int @ 14:19", "var y int @ 14:19",
        "*ast.CompositeLit @ 14:6", "var a [2]int @ 14:2", "var a [2]int @ 15:6",
        "var y int @ 16:19", "var y int @ 16:19", "*ast.CompositeLit @ 16:7",
        "var b [4]any @ 16:2", "var b [4]any @ 17:6",
        // []any{y, z}: slot stores, then the Slice store and the read
        "var y int @ 18:13", "var y int @ 18:13", "var z error @ 18:16", "var z error @ 18:16",
        "*ast.CompositeLit @ 18:7", "*ast.CompositeLit @ 18:7",
        "var c []any @ 18:2", "var c []any @ 19:6",
        // map: an element slot records nothing; the MakeMap store does
        "var y int @ 20:22", "*ast.CompositeLit @ 20:7", "*ast.CompositeLit @ 20:7",
        "var m map[int]any @ 20:2", "var m map[int]any @ 21:6",
        "*ast.BasicLit @ 22:13", "address of *ast.CompositeLit @ 22:8", "*ast.UnaryExpr @ 22:7",
        "var q *S @ 22:2", "var q *S @ 23:6",
        "var y int @ 24:15", "var y int @ 24:15", "*ast.CompositeLit @ 24:11",
        "*ast.CompositeLit @ 24:11", "*ast.CompositeLit @ 24:7", "*ast.CompositeLit @ 24:7",
        "var t []S @ 24:2", "var t []S @ 25:6",
        // [3]int{y}: a partial literal (its memclear is unreachable in v0.50:
        // compLit is only called on a fresh, zeroed Alloc)
        "var ar [3]int @ 26:6", "var y int @ 27:14", "var y int @ 27:14",
        "*ast.CompositeLit @ 27:7", "var ar [3]int @ 27:2", "var ar [3]int @ 28:6",
        "var w any @ 29:6", "var y int @ 30:6", "var w any @ 30:2", "var w any @ 31:6",
        // []*S{{f: y}}: v0.52 builds the implied `&S{…}` in `expr`, whose
        // DebugRef names the pointer before the slot store's
        "var y int @ 32:16", "var y int @ 32:16", "address of *ast.CompositeLit @ 32:12",
        "*ast.CompositeLit @ 32:12", "*ast.CompositeLit @ 32:12",
        "*ast.CompositeLit @ 32:7", "*ast.CompositeLit @ 32:7",
        "var n []*S @ 32:2", "var n []*S @ 33:6",
    ];
    assert_eq!(descrs, want, "DebugRefs; full disassembly:\n{asm}");

    // A store into an `any` slot names the MakeInterface, not the int read
    // (ssadump: `var y int @ 11:17 is t3` after `… is y`).
    let value_of = |d: &str, nth: usize| {
        refs.iter().filter(|(x, _)| *x == d).nth(nth).map(|(_, v)| *v).unwrap()
    };
    assert_ne!(value_of("var y int @ 11:17", 0), value_of("var y int @ 11:17", 1), "{asm}");
    assert_eq!(value_of("var y int @ 11:11", 0), value_of("var y int @ 11:11", 1), "{asm}");
    assert_ne!(value_of("var w any @ 30:2", 0), value_of("var y int @ 30:6", 0), "{asm}");
}

/// Under `IR_IN_PLACE_PARTIAL_ARRAYS` (buildir, what staticcheck reads) a
/// partial array literal assigned to a variable fills that variable in place,
/// as honnef's IR (go/ir v0.8.2 `builder.assign`) does: the cell's address
/// DebugRef, the memclear unless the cell is freshly defined, the element
/// stores, then the literal recorded *as the cell's address*. Everything else
/// still copies through a temporary. Address-or-value of the first DebugRef of
/// each literal matches honnef's `fn.ValueForExpr` on this source, measured with
/// `irutil.Packages(…, ir.GlobalDebug)`; a `:=` that only redeclares `c`
/// clears it first (honnef's `assignLHS` reports `isZero` for defined
/// variables only).
#[test]
fn test_partial_array_literal_in_place_under_ir_mode() {
    const SRC: &str = "package p

type S struct{ arr [3]int }

func f(y int) {
	var ar [3]int
	ar = [3]int{y}
	_ = ar
	b := [...]any{3: y}
	_ = b
	c := [3]int{1, 2, 3}
	n, c := 0, [3]int{y}
	_, _ = n, c
	full := [3]int{y, y, y}
	_ = full
	s := S{arr: [3]int{y}}
	_ = s
	nest := [2][3]int{{y}}
	_ = nest
}";
    let asm = disasm_debug_mode(
        SRC,
        BuilderMode::GLOBAL_DEBUG | BuilderMode::IR_IN_PLACE_PARTIAL_ARRAYS,
    );
    let lits: Vec<&str> = asm
        .lines()
        .filter_map(|l| l.split_once("; ").map(|(_, r)| r))
        .filter(|r| r.contains("*ast.CompositeLit @ "))
        .filter_map(|r| r.split_once(" is ").map(|(d, _)| d))
        .collect();
    assert_eq!(
        lits,
        [
            "address of *ast.CompositeLit @ 7:7",   // ar = [3]int{y}
            "address of *ast.CompositeLit @ 9:7",   // b := [...]any{3: y}
            "*ast.CompositeLit @ 11:7",             // c := full: copied
            "address of *ast.CompositeLit @ 12:13", // n, c := 0, [3]int{y}
            "*ast.CompositeLit @ 14:10",            // full: copied
            // struct field slot: copied (the read, then the slot's store)
            "*ast.CompositeLit @ 16:14",
            "*ast.CompositeLit @ 16:14",
            "*ast.CompositeLit @ 16:7",
            // inner {y}, in place: the element slot's address, then the literal
            "address of *ast.CompositeLit @ 18:20",
            "address of *ast.CompositeLit @ 18:20",
            "address of *ast.CompositeLit @ 18:10", // nest, in place
        ],
        "{asm}"
    );
    // Each in-place literal names its variable's cell; a defined `b` and
    // `nest` skip the memclear, the existing `ar` and redeclared `c` do not.
    let zero_stores = asm.lines().filter(|l| l.contains(" = [3]int{}")).count();
    assert_eq!(zero_stores, 2, "{asm}");
    assert!(!asm.contains(" = [4]any{}"), "{asm}");
    assert!(!asm.contains(" = [2][3]int{}"), "{asm}");

    // Without the mode (go/ssa, x/tools v0.52) nothing is filled in place.
    let plain = disasm_debug(SRC);
    assert!(!plain.contains("address of *ast.CompositeLit"), "{plain}");
}
