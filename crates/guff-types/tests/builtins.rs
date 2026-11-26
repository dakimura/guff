//! Tests for `builtins.rs` (chunk 29a) — `len` / `cap` / `append` / `copy`.
//!
//! Builtins resolve through the universe scope, so a call with `fun = ident`
//! (e.g. `len`) drives `Checker::call_expr` → `Checker::builtin`. Operand types
//! (slices/arrays/strings) are built directly in the arenas.

use guff::ast::{BasicLit, CallExpr, Expr, Ident};
use guff::position::Pos;
use guff::token::Token;

use guff_constant::int64_val;
use guff_types::array::new_array;
use guff_types::basic::BasicKind;
use guff_types::chan::{new_chan, ChanDir};
use guff_types::map::new_map;
use guff_types::object::type_name::new_type_name;
use guff_types::object::var::new_var;
use guff_types::operand::OperandMode;
use guff_types::scope::insert as scope_insert;
use guff_types::slice::new_slice;
use guff_types::{Checker, Config, Operand, TypeId, TypeKind};

fn checker() -> Checker {
    let mut check = Checker::new(Config::default());
    let pkg_scope = check.packages.get(check.pkg).scope();
    check.env.scope = Some(pkg_scope);
    check
}

fn add_var(check: &mut Checker, name: &str, typ: TypeId) {
    let pkg_scope = check.packages.get(check.pkg).scope();
    let v = new_var(&mut check.objects, name, typ);
    scope_insert(&mut check.scopes, &mut check.objects, pkg_scope, v);
}

/// Declare a package-scope type name `name` denoting `typ`.
fn add_type(check: &mut Checker, name: &str, typ: TypeId) {
    let pkg_scope = check.packages.get(check.pkg).scope();
    let tn = new_type_name(&mut check.objects, name, Some(typ));
    scope_insert(&mut check.scopes, &mut check.objects, pkg_scope, tn);
}

fn ident(name: &str) -> Expr {
    Expr::Ident(Ident {
        name: name.to_string(),
        ..Default::default()
    })
}

fn int_lit(v: &str) -> Expr {
    Expr::BasicLit(BasicLit {
        id: 0,
        value_pos: Pos(1),
        value_end: Pos(1),
        kind: Some(Token::INT),
        value: v.to_string(),
    })
}

fn call(fun: &str, args: Vec<Expr>) -> Expr {
    Expr::CallExpr(CallExpr {
        id: 0,
        fun: Box::new(ident(fun)),
        lparen: Pos(1),
        args,
        ellipsis: Pos(0),
        rparen: Pos(1),
    })
}

#[test]
fn len_of_slice_is_value_int() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let slice_t = new_slice(&mut check.types, int_t);
    add_var(&mut check, "s", slice_t);

    let c = call("len", vec![ident("s")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap(), int_t);
}

#[test]
fn len_of_array_is_constant() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let arr_t = new_array(&mut check.types, int_t, 3);
    add_var(&mut check, "a", arr_t);

    let c = call("len", vec![ident("a")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    // len of an array (no calls/receives) is a constant.
    assert_eq!(op.mode, OperandMode::Constant);
    let (v, ok) = int64_val(op.val.as_ref().unwrap());
    assert!(ok);
    assert_eq!(v, 3);
}

#[test]
fn cap_of_slice_is_value_int() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let slice_t = new_slice(&mut check.types, int_t);
    add_var(&mut check, "s", slice_t);

    let c = call("cap", vec![ident("s")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap(), int_t);
}

#[test]
fn append_returns_slice_type() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let slice_t = new_slice(&mut check.types, int_t);
    add_var(&mut check, "s", slice_t);

    // append(s, 1)
    let c = call("append", vec![ident("s"), int_lit("1")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap(), slice_t);
    assert_eq!(op.typ.unwrap().kind(&check.types), TypeKind::Slice);
}

#[test]
fn copy_returns_int() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let slice_t = new_slice(&mut check.types, int_t);
    add_var(&mut check, "dst", slice_t);
    add_var(&mut check, "src", slice_t);

    let c = call("copy", vec![ident("dst"), ident("src")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap(), int_t);
}

#[test]
fn len_of_non_lenable_errors() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    add_var(&mut check, "x", int_t);

    let c = call("len", vec![ident("x")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert_eq!(op.mode, OperandMode::Invalid);
    assert!(
        !check.errors.is_empty(),
        "len of an int must report an error"
    );
}

#[test]
fn dotdotdot_on_non_append_errors() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let slice_t = new_slice(&mut check.types, int_t);
    add_var(&mut check, "s", slice_t);

    // len(s...) — `...` is only valid for append.
    let mut c = call("len", vec![ident("s")]);
    if let Expr::CallExpr(ref mut call) = c {
        call.ellipsis = Pos(5);
    }
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert_eq!(op.mode, OperandMode::Invalid);
    assert!(
        !check.errors.is_empty(),
        "... with a non-append builtin must report an error"
    );
}

// ---------------------------------------------------------------------------
// chunk 29b — make / new / delete / clear

#[test]
fn make_slice_yields_the_type() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let slice_t = new_slice(&mut check.types, int_t);
    add_type(&mut check, "S", slice_t);

    // make(S, 3)
    let c = call("make", vec![ident("S"), int_lit("3")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap(), slice_t);
}

#[test]
fn make_map_one_arg() {
    let mut check = checker();
    let str_t = check.basic(BasicKind::String);
    let int_t = check.basic(BasicKind::Int);
    let map_t = new_map(&mut check.types, str_t, int_t);
    add_type(&mut check, "M", map_t);

    let c = call("make", vec![ident("M")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap(), map_t);
}

#[test]
fn make_of_non_makeable_errors() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    add_type(&mut check, "T", int_t);

    let c = call("make", vec![ident("T"), int_lit("1")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(
        !check.errors.is_empty(),
        "make of a non-slice/map/chan must report an error"
    );
}

#[test]
fn make_swapped_len_cap_errors() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let slice_t = new_slice(&mut check.types, int_t);
    add_type(&mut check, "S", slice_t);

    // make(S, 3, 1) — length > capacity.
    let c = call("make", vec![ident("S"), int_lit("3"), int_lit("1")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(
        !check.errors.is_empty(),
        "make with length > capacity must report an error"
    );
}

#[test]
fn new_yields_pointer() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    add_type(&mut check, "T", int_t);

    let c = call("new", vec![ident("T")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap().kind(&check.types), TypeKind::Pointer);
}

#[test]
fn delete_is_no_value() {
    let mut check = checker();
    let str_t = check.basic(BasicKind::String);
    let int_t = check.basic(BasicKind::Int);
    let map_t = new_map(&mut check.types, str_t, int_t);
    add_var(&mut check, "m", map_t);

    let c = call("delete", vec![ident("m"), str_lit("k")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::NoValue);
}

#[test]
fn clear_is_no_value() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let slice_t = new_slice(&mut check.types, int_t);
    add_var(&mut check, "s", slice_t);

    let c = call("clear", vec![ident("s")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::NoValue);
}

fn str_lit(v: &str) -> Expr {
    Expr::BasicLit(BasicLit {
        id: 0,
        value_pos: Pos(1),
        value_end: Pos(1),
        kind: Some(Token::STRING),
        value: format!("\"{}\"", v),
    })
}

#[test]
fn copy_mismatched_element_types_errors() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let str_t = check.basic(BasicKind::String);
    let int_slice = new_slice(&mut check.types, int_t);
    let str_slice = new_slice(&mut check.types, str_t);
    add_var(&mut check, "dst", int_slice);
    add_var(&mut check, "src", str_slice);

    let c = call("copy", vec![ident("dst"), ident("src")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(
        !check.errors.is_empty(),
        "copy between []int and []string must report an error"
    );
}

// ---------------------------------------------------------------------------
// chunk 29c — close / complex / real / imag / min / max / panic / recover / print

#[test]
fn close_is_no_value() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let ch_t = new_chan(&mut check.types, ChanDir::SendRecv, int_t);
    add_var(&mut check, "ch", ch_t);

    let c = call("close", vec![ident("ch")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::NoValue);
}

#[test]
fn close_non_channel_errors() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    add_var(&mut check, "x", int_t);

    let c = call("close", vec![ident("x")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(!check.errors.is_empty(), "close of non-channel must error");
}

#[test]
fn complex_of_untyped_constants() {
    let mut check = checker();
    let c = call("complex", vec![int_lit("1"), int_lit("2")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Constant);
    assert_eq!(op.typ.unwrap(), check.basic(BasicKind::UntypedComplex));
}

#[test]
fn real_of_complex_var() {
    let mut check = checker();
    let c128 = check.basic(BasicKind::Complex128);
    add_var(&mut check, "z", c128);

    let c = call("real", vec![ident("z")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap(), check.basic(BasicKind::Float64));
}

#[test]
fn imag_of_complex_var() {
    let mut check = checker();
    let c64 = check.basic(BasicKind::Complex64);
    add_var(&mut check, "z", c64);

    let c = call("imag", vec![ident("z")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap(), check.basic(BasicKind::Float32));
}

#[test]
fn min_of_constants_folds() {
    let mut check = checker();
    let c = call("min", vec![int_lit("1"), int_lit("2")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Constant);
    let (v, ok) = int64_val(op.val.as_ref().unwrap());
    assert!(ok);
    assert_eq!(v, 1);
}

#[test]
fn max_of_constants_folds() {
    let mut check = checker();
    let c = call("max", vec![int_lit("1"), int_lit("2")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Constant);
    let (v, ok) = int64_val(op.val.as_ref().unwrap());
    assert!(ok);
    assert_eq!(v, 2);
}

#[test]
fn min_of_unordered_errors() {
    let mut check = checker();
    let bool_t = check.basic(BasicKind::Bool);
    add_var(&mut check, "b", bool_t);

    let c = call("min", vec![ident("b")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(!check.errors.is_empty(), "min of a bool must error");
}

#[test]
fn panic_is_no_value() {
    let mut check = checker();
    let c = call("panic", vec![str_lit("boom")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::NoValue);
}

#[test]
fn recover_yields_interface() {
    let mut check = checker();
    let c = call("recover", vec![]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap(), check.universe_any);
}

#[test]
fn print_is_no_value() {
    let mut check = checker();
    let c = call("print", vec![int_lit("1"), str_lit("x")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::NoValue);
}
