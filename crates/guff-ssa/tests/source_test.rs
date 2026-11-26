//! source.rs tests (Milestone D, chunk D06).
//!
//! Exercises `Function::value_for_expr` (source expression → SSA value, via the
//! DebugRefs emitted in debug mode) and `Program::package` (type-checker
//! package → SSA package).

use guff::ast::{Decl, Expr, File, Stmt};
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::Builder;
use guff_ssa::ids::FuncId;
use guff_ssa::instr::InstrData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_ssa::value::Value;
use guff_types::{Checker, Config};

const SRC: &str = "package p\nfunc f(x int) int {\n\tx = x + 1\n\treturn x\n}";

/// Builds `SRC`'s `f` under `mode`, returning the finished program, func id,
/// the type-checker package id, and the parsed file (for AST navigation).
fn build(mode: BuilderMode) -> (Program, FuncId, guff_types::PackageId, File) {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", SRC.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);

    let type_pkg_id = check.pkg;

    let mut prog = Program::new(mode, check.info, check.types, check.objects, check.packages);
    prog.set_fset(fset.clone());

    let ssa_pkg_id = guff_ssa::create::create_package(&mut prog, type_pkg_id);

    let fd = file
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::FuncDecl(fd) => Some(fd),
            _ => None,
        })
        .expect("no FuncDecl");

    let fn_id =
        guff_ssa::create::create_function(&mut prog, fd.name.name.clone(), None, Some(ssa_pkg_id));

    // Register parameters from the signature (create::create_params).
    let obj_f = prog.info.defs.get(&fd.name.id).unwrap().unwrap();
    let sig_id = obj_f.typ(&prog.object_arena).unwrap();
    prog.functions.get_mut(fn_id).signature = Some(sig_id);
    guff_ssa::create::create_params(&mut prog, fn_id);

    let mut builder = Builder::new(&mut prog, fn_id);
    let entry = builder.new_basic_block("entry".to_string());
    builder.set_block(Some(entry));
    builder.stmt(&Stmt::BlockStmt(fd.body.clone().unwrap()));
    drop(builder);

    (prog, fn_id, type_pkg_id, file)
}

/// Returns (`x + 1` BinaryExpr, `return x` Ident) from the parsed body of `f`.
fn body_exprs(file: &File) -> (Expr, Expr) {
    let fd = file
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::FuncDecl(fd) => Some(fd),
            _ => None,
        })
        .unwrap();
    let body = &fd.body.as_ref().unwrap().list;

    let binexpr = body
        .iter()
        .find_map(|s| match s {
            Stmt::AssignStmt(a) => Some(a.rhs[0].clone()),
            _ => None,
        })
        .expect("no assign rhs");
    assert!(matches!(binexpr, Expr::BinaryExpr(_)), "rhs should be a BinaryExpr");

    let ret_ident = body
        .iter()
        .find_map(|s| match s {
            Stmt::ReturnStmt(r) => Some(r.results[0].clone()),
            _ => None,
        })
        .expect("no return result");
    assert!(matches!(ret_ident, Expr::Ident(_)), "return result should be an Ident");

    (binexpr, ret_ident)
}

#[test]
fn test_value_for_expr_binaryexpr_resolves_to_binop() {
    let (prog, fn_id, _, file) = build(BuilderMode::GLOBAL_DEBUG);
    let (binexpr, _) = body_exprs(&file);
    let f = prog.functions.get(fn_id);

    let (v, is_addr) = f
        .value_for_expr(&binexpr)
        .expect("value_for_expr should find the `x + 1` DebugRef");
    assert!(!is_addr, "the value of a BinaryExpr is not an address");
    match v {
        Value::Instr(id) => assert!(
            matches!(f.instrs.get(id), InstrData::BinOp(_)),
            "the `x + 1` expression should resolve to a BinOp instruction"
        ),
        other => panic!("expected an instruction value, got {other:?}"),
    }
}

#[test]
fn test_value_for_expr_return_ident_resolves() {
    let (prog, fn_id, _, file) = build(BuilderMode::GLOBAL_DEBUG);
    let (_, ret_ident) = body_exprs(&file);
    let f = prog.functions.get(fn_id);

    let (_, is_addr) = f
        .value_for_expr(&ret_ident)
        .expect("value_for_expr should find the return `x` DebugRef");
    assert!(!is_addr, "the return `x` is read for its value, not its address");
}

#[test]
fn test_value_for_expr_none_without_debug() {
    // Without GLOBAL_DEBUG no DebugRefs are emitted, so nothing is recoverable.
    let (prog, fn_id, _, file) = build(BuilderMode::default());
    let (binexpr, ret_ident) = body_exprs(&file);
    let f = prog.functions.get(fn_id);

    assert!(f.value_for_expr(&binexpr).is_none());
    assert!(f.value_for_expr(&ret_ident).is_none());
}

// --- package-level named-entity lookups (D08) ---

const PKG_SRC: &str = "\
package p

const C = 42

var V int

func F() {}
";

/// Parses `PKG_SRC`, type-checks it, creates the SSA package and populates its
/// members. Returns the program and the type-checker package id.
fn build_pkg() -> (Program, guff_types::PackageId) {
    let fset = FileSet::new();
    let pfile = parse_file(&fset, "pkg.go", PKG_SRC.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![pfile.clone()]);

    let type_pkg_id = check.pkg;
    let mut prog = Program::new(
        BuilderMode::default(),
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    let ssa_pkg_id = guff_ssa::create::create_package(&mut prog, type_pkg_id);
    guff_ssa::create::populate_package_members(&mut prog, ssa_pkg_id, &[pfile]);
    (prog, type_pkg_id)
}

/// Looks up the type-checker object defined at top level under `name`, by
/// scanning `Info.defs` (the program carries no scope arena). Names are unique
/// among `PKG_SRC`'s top-level declarations.
fn top_level_object(prog: &Program, type_pkg_id: guff_types::PackageId, name: &str) -> guff_types::ObjectId {
    prog.info
        .defs
        .values()
        .flatten()
        .copied()
        .find(|&o| {
            o.name(&prog.object_arena) == name && o.pkg(&prog.object_arena) == Some(type_pkg_id)
        })
        .unwrap_or_else(|| panic!("no top-level object named {name}"))
}

#[test]
fn test_func_value() {
    let (prog, type_pkg_id) = build_pkg();
    let obj_f = top_level_object(&prog, type_pkg_id, "F");
    let fid = prog.func_value(obj_f).expect("F should resolve to a Function");
    assert_eq!(prog.functions.get(fid).name, "F");
}

#[test]
fn test_package_level_member_var_is_global() {
    let (prog, type_pkg_id) = build_pkg();
    let obj_v = top_level_object(&prog, type_pkg_id, "V");
    match prog.package_level_member(obj_v) {
        Some(Value::Global(_)) => {}
        other => panic!("V should be a package-level Global, got {other:?}"),
    }
    // A var is not a function.
    assert!(prog.func_value(obj_v).is_none());
}

#[test]
fn test_const_value() {
    let (prog, type_pkg_id) = build_pkg();
    let obj_c = top_level_object(&prog, type_pkg_id, "C");
    let c = prog.const_value(obj_c).expect("C should resolve to a Const");
    let val = c.val.expect("named const C should have a value");
    let (n, exact) = guff_constant::int64_val(&val);
    assert!(exact, "42 is exactly representable as i64");
    assert_eq!(n, 42);
}

// --- path-based enclosing-function lookups (D09) ---

/// Returns the SSA package id and the `func F` declaration Node + File Node,
/// so tests can assemble an enclosing-interval `path`.
fn build_pkg_with_path() -> (Program, guff_ssa::ids::PackageId, Vec<guff::ast::Node>) {
    use guff::ast::{Decl, Node};

    let fset = FileSet::new();
    let pfile = parse_file(&fset, "pkg.go", PKG_SRC.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![pfile.clone()]);
    let type_pkg_id = check.pkg;

    let mut prog = Program::new(
        BuilderMode::default(),
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    let ssa_pkg_id = guff_ssa::create::create_package(&mut prog, type_pkg_id);
    guff_ssa::create::populate_package_members(&mut prog, ssa_pkg_id, &[pfile.clone()]);

    let fdecl = pfile
        .decls
        .iter()
        .find(|d| matches!(d, Decl::FuncDecl(fd) if fd.name.name == "F"))
        .expect("no func F")
        .clone();

    // Path innermost-first: [FuncDecl, File]  (path[n-2] is the enclosing decl).
    let path = vec![Node::Decl(fdecl), Node::File(Box::new(pfile))];
    (prog, ssa_pkg_id, path)
}

#[test]
fn test_enclosing_function_package_level() {
    let (mut prog, pkg_id, path) = build_pkg_with_path();
    let fid = prog
        .enclosing_function(pkg_id, &path)
        .expect("F's declaration should be enclosed by function F");
    assert_eq!(prog.functions.get(fid).name, "F");
    assert!(prog.has_enclosing_function(pkg_id, &path));
}

#[test]
fn test_find_named_func_by_pos() {
    use guff::ast::Decl;
    let (mut prog, pkg_id, path) = build_pkg_with_path();
    // Extract F's name position from the FuncDecl in the path.
    let pos = match &path[0] {
        guff::ast::Node::Decl(Decl::FuncDecl(fd)) => fd.name.name_pos.0 as u32,
        _ => unreachable!(),
    };
    let fid = prog.find_named_func(pkg_id, pos).expect("F should be found by pos");
    assert_eq!(prog.functions.get(fid).name, "F");

    // A position that matches no declaration finds nothing.
    assert!(prog.find_named_func(pkg_id, 999_999).is_none());
}

#[test]
fn test_enclosing_function_none_for_short_path() {
    let (mut prog, pkg_id, path) = build_pkg_with_path();
    // A path with only the File node (n < 2) is not inside any function.
    let file_only = &path[1..];
    assert!(prog.enclosing_function(pkg_id, file_only).is_none());
    assert!(!prog.has_enclosing_function(pkg_id, file_only));
}

// --- VarValue (D10) ---

/// Builds `SRC`'s `f` the go/ssa way: the function is created as a package
/// member (so it carries its object and is discoverable by
/// `enclosing_function`), then its body is built in debug mode (so DebugRefs
/// exist). Returns the program, ssa package id, func id, and the parsed file.
fn build_f_member_debug() -> (Program, guff_ssa::ids::PackageId, FuncId, File) {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", SRC.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    let type_pkg_id = check.pkg;

    let mut prog = Program::new(
        BuilderMode::GLOBAL_DEBUG,
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    prog.set_fset(fset.clone());

    let ssa_pkg_id = guff_ssa::create::create_package(&mut prog, type_pkg_id);
    guff_ssa::create::populate_package_members(&mut prog, ssa_pkg_id, &[file.clone()]);

    let fn_id = match prog.packages.get(ssa_pkg_id).members.get("f") {
        Some(guff_ssa::member::MemberData::Function(fid)) => *fid,
        other => panic!("f should be a package-level Function member, got {other:?}"),
    };

    // Register SSA parameters (populate_package_members creates the function
    // shell but not its params; the builder needs them in f.objects).
    guff_ssa::create::create_params(&mut prog, fn_id);

    let fd = file
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::FuncDecl(fd) => Some(fd),
            _ => None,
        })
        .unwrap();

    let mut builder = Builder::new(&mut prog, fn_id);
    let entry = builder.new_basic_block("entry".to_string());
    builder.set_block(Some(entry));
    builder.stmt(&Stmt::BlockStmt(fd.body.clone().unwrap()));
    drop(builder);

    (prog, ssa_pkg_id, fn_id, file)
}

/// Extracts the `func f` declaration Node from a file (for use in a path).
fn func_decl_node(file: &File) -> guff::ast::Node {
    let decl = file
        .decls
        .iter()
        .find(|d| matches!(d, Decl::FuncDecl(fd) if fd.name.name == "f"))
        .expect("no func f")
        .clone();
    guff::ast::Node::Decl(decl)
}

#[test]
fn test_var_value_debugref_branch() {
    use guff::ast::Node;

    let (mut prog, pkg_id, _fid, file) = build_f_member_debug();

    // The `x` in `return x` (line 4): a use of the param var.
    let fd = file.decls.iter().find_map(|d| match d {
        Decl::FuncDecl(fd) => Some(fd),
        _ => None,
    }).unwrap();
    let ret_ident = fd.body.as_ref().unwrap().list.iter().find_map(|s| match s {
        Stmt::ReturnStmt(r) => Some(r.results[0].clone()),
        _ => None,
    }).unwrap();
    let ret_id = match &ret_ident {
        Expr::Ident(id) => id.clone(),
        _ => unreachable!(),
    };
    // The object the ident denotes (a use -> Info.uses).
    let obj = prog.info.uses.get(&ret_id.id).copied().expect("return x resolves to an object");

    let path = vec![Node::Expr(ret_ident.clone()), func_decl_node(&file), Node::File(Box::new(file.clone()))];
    let (v, is_addr) = prog.var_value(obj, pkg_id, &path).expect("var_value should resolve return x");
    assert!(!is_addr, "the return `x` DebugRef records a value");
    // It should be a real SSA value (register or param), not nothing.
    assert!(matches!(v, Value::Instr(_) | Value::Param(_) | Value::Const(_)), "got {v:?}");
}

#[test]
fn test_var_value_parameter_branch() {
    use guff::ast::Node;

    let (mut prog, pkg_id, _fid, file) = build_f_member_debug();

    // The defining ident `x` in the parameter list `f(x int)`.
    let fd = file.decls.iter().find_map(|d| match d {
        Decl::FuncDecl(fd) => Some(fd),
        _ => None,
    }).unwrap();
    let param_ident = fd.ty.params.as_ref().unwrap().list[0].names[0].clone();
    let obj = prog.info.defs.get(&param_ident.id).copied().flatten().expect("param x is a def");

    let path = vec![
        Node::Expr(Expr::Ident(param_ident.clone())),
        func_decl_node(&file),
        Node::File(Box::new(file.clone())),
    ];
    let (v, is_addr) = prog.var_value(obj, pkg_id, &path).expect("var_value should resolve the param def");
    assert!(!is_addr, "a parameter value is not an address");
    assert!(matches!(v, Value::Param(_)), "defining ident of a param should yield a Parameter, got {v:?}");
}

#[test]
fn test_program_package_lookup() {
    use std::num::NonZeroU32;

    let (prog, _, type_pkg_id, _) = build(BuilderMode::GLOBAL_DEBUG);

    // The checked package has an SSA package created for it.
    assert!(
        prog.package(type_pkg_id).is_some(),
        "the type-checker package should map to an SSA package"
    );

    // An unknown type-checker package maps to nothing.
    let bogus = unsafe {
        std::mem::transmute::<NonZeroU32, guff_types::PackageId>(NonZeroU32::new(9999).unwrap())
    };
    assert!(prog.package(bogus).is_none());
}
