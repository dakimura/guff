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

    // Collect DebugRefs. Expect three: `x` (RHS var read), the `x + 1`
    // BinaryExpr, and `x` again in the return.
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
    assert_eq!(var_lines, vec![3, 4], "expected var-`x` DebugRefs on lines 3 and 4");
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
