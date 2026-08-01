//! Tests for `expr.rs` (chunk 25a) — identifier expression resolution.

use guff::ast::{Expr, Ident};
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::operand::OperandMode;
use guff_types::{Checker, Config, Operand, TypeKind};

fn parse(src: &str) -> guff::ast::File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

/// Collect objects from `src` and set the lookup scope to the package scope.
fn checker(src: &str) -> Checker {
    let mut check = Checker::new(Config::default());
    check.files = vec![parse(src)];
    check.collect_objects();
    let pkg_scope = check.packages.get(check.pkg).scope();
    check.env.scope = Some(pkg_scope);
    check
}

/// Build a bare identifier expression node.
fn ident(name: &str) -> Ident {
    Ident {
        name: name.to_string(),
        ..Default::default()
    }
}

// Package-level vars start with a Typ[Invalid] placeholder; `ident` forces
// `obj_decl` when that placeholder is still present (Go: typ == nil).

#[test]
fn ident_of_type_name() {
    let mut check = checker("package p\ntype T int\n");
    let mut x = Operand::invalid();
    check.ident(&mut x, &ident("T"), false);
    assert_eq!(x.mode, OperandMode::TypeExpr);
    // obj_decl was forced: T resolves to a Named over int.
    assert_eq!(x.typ.unwrap().kind(&check.types), TypeKind::Named);
}

#[test]
fn ident_of_func() {
    let mut check = checker("package p\nfunc f() {}\n");
    let mut x = Operand::invalid();
    check.ident(&mut x, &ident("f"), false);
    assert_eq!(x.mode, OperandMode::Value);
    assert_eq!(x.typ.unwrap().kind(&check.types), TypeKind::Signature);
}

#[test]
fn ident_of_universe_type_int() {
    let mut check = checker("package p\n");
    let mut x = Operand::invalid();
    check.ident(&mut x, &ident("int"), true);
    assert_eq!(x.mode, OperandMode::TypeExpr);
    assert_eq!(x.typ.unwrap().kind(&check.types), TypeKind::Basic);
}

#[test]
fn ident_of_universe_const_true() {
    let mut check = checker("package p\n");
    let mut x = Operand::invalid();
    check.ident(&mut x, &ident("true"), false);
    assert_eq!(x.mode, OperandMode::Constant);
    assert!(x.val.is_some());
}

#[test]
fn ident_of_builtin_len() {
    let mut check = checker("package p\n");
    let mut x = Operand::invalid();
    check.ident(&mut x, &ident("len"), false);
    assert_eq!(x.mode, OperandMode::Builtin);
    assert!(x.id.is_some());
}

#[test]
fn undefined_ident_reports_error() {
    let mut check = checker("package p\n");
    let mut x = Operand::invalid();
    check.ident(&mut x, &ident("nope"), false);
    assert_eq!(x.mode, OperandMode::Invalid);
    assert!(check
        .errors
        .iter()
        .any(|e| e.msg.contains("undefined: nope")));
}

#[test]
fn iota_outside_const_decl_errors() {
    let mut check = checker("package p\n");
    let mut x = Operand::invalid();
    check.ident(&mut x, &ident("iota"), false);
    assert!(check.errors.iter().any(|e| e.msg.contains("iota")));
}

#[test]
fn expr_dispatches_ident() {
    let mut check = checker("package p\nfunc f() {}\n");
    let e = Expr::Ident(ident("f"));
    let mut x = Operand::invalid();
    check.expr(&mut x, &e);
    assert_eq!(x.mode, OperandMode::Value);
}

// ---- chunk 25b: basic literals + unary ----

use guff::parser::parse_expr_from;
use guff_constant::{int64_val, Value};

/// Parse a standalone expression.
fn parse_expr(src: &str) -> Expr {
    let fset = FileSet::new();
    parse_expr_from(&fset, "expr.go", src.as_bytes(), Mode::NONE).expect("parse expr")
}

/// Snapshot of an evaluated expression (Operand borrows the AST, so tests
/// cannot return the Operand itself alongside a dropped local Expr).
struct Eval {
    check: Checker,
    mode: OperandMode,
    typ: Option<guff_types::TypeId>,
    val: Option<Value>,
}

/// Evaluate `src` as an expression in an empty package.
fn eval(src: &str) -> Eval {
    let mut check = checker("package p\n");
    let e = parse_expr(src);
    let mut x = Operand::invalid();
    check.expr(&mut x, &e);
    Eval {
        check,
        mode: x.mode,
        typ: x.typ,
        val: x.val.clone(),
    }
}

fn as_i64(v: &Value) -> i64 {
    int64_val(v).0
}

#[test]
fn int_literal_is_untyped_constant() {
    let ev = eval("42");
    assert_eq!(ev.mode, OperandMode::Constant);
    assert_eq!(as_i64(ev.val.as_ref().unwrap()), 42);
    assert!(ev.check.errors.is_empty());
}

#[test]
fn string_literal_constant() {
    let ev = eval("\"hi\"");
    assert_eq!(ev.mode, OperandMode::Constant);
}

#[test]
fn unary_negate_constant() {
    let ev = eval("-7");
    assert_eq!(ev.mode, OperandMode::Constant);
    assert_eq!(as_i64(ev.val.as_ref().unwrap()), -7);
}

#[test]
fn unary_complement_constant() {
    let ev = eval("^0");
    assert_eq!(ev.mode, OperandMode::Constant);
    assert_eq!(as_i64(ev.val.as_ref().unwrap()), -1);
}

#[test]
fn unary_not_on_bool_constant() {
    let ev = eval("!true");
    assert_eq!(ev.mode, OperandMode::Constant);
    // !true == false
    assert!(matches!(ev.val, Some(Value::Bool(false))));
}

#[test]
fn unary_not_on_int_is_error() {
    let ev = eval("!5");
    assert_eq!(ev.mode, OperandMode::Invalid);
    assert!(!ev.check.errors.is_empty());
}

#[test]
fn paren_expr_unwraps() {
    let ev = eval("(-3)");
    assert_eq!(ev.mode, OperandMode::Constant);
    assert_eq!(as_i64(ev.val.as_ref().unwrap()), -3);
}

// ---- chunk 25c: binary / comparison / shift ----

#[test]
fn binary_add_constants() {
    let ev = eval("1 + 2");
    assert_eq!(ev.mode, OperandMode::Constant);
    assert_eq!(as_i64(ev.val.as_ref().unwrap()), 3);
    assert!(ev.check.errors.is_empty());
}

#[test]
fn binary_mixed_int_float() {
    // untyped int + untyped float => untyped float 3.5
    let ev = eval("1 + 2.5");
    assert_eq!(ev.mode, OperandMode::Constant);
    assert!(ev.check.errors.is_empty());
}

#[test]
fn integer_division_is_integer() {
    let ev = eval("7 / 2");
    assert_eq!(ev.mode, OperandMode::Constant);
    assert_eq!(as_i64(ev.val.as_ref().unwrap()), 3);
}

#[test]
fn division_by_zero_errors() {
    let ev = eval("1 / 0");
    assert_eq!(ev.mode, OperandMode::Invalid);
    assert!(ev
        .check
        .errors
        .iter()
        .any(|e| e.msg.contains("division by zero")));
}

#[test]
fn string_concat() {
    let ev = eval("\"a\" + \"b\"");
    assert_eq!(ev.mode, OperandMode::Constant);
    assert!(matches!(ev.val, Some(Value::String(_))));
}

#[test]
fn comparison_yields_bool_constant() {
    let ev = eval("3 < 5");
    assert_eq!(ev.mode, OperandMode::Constant);
    assert!(matches!(ev.val, Some(Value::Bool(true))));
    assert_eq!(ev.typ.unwrap().kind(&ev.check.types), TypeKind::Basic);
}

#[test]
fn equality_constant() {
    let ev = eval("2 == 2");
    assert_eq!(ev.mode, OperandMode::Constant);
    assert!(matches!(ev.val, Some(Value::Bool(true))));
}

#[test]
fn shift_constant() {
    let ev = eval("1 << 4");
    assert_eq!(ev.mode, OperandMode::Constant);
    assert_eq!(as_i64(ev.val.as_ref().unwrap()), 16);
}

#[test]
fn add_bool_is_error() {
    let ev = eval("true + false");
    assert_eq!(ev.mode, OperandMode::Invalid);
    assert!(!ev.check.errors.is_empty());
}
