//! Tests for `index.rs` (chunk 28) — index `a[i]` and slice `a[lo:hi]`
//! expressions.
//!
//! Type expressions for arrays/slices/maps aren't supported by `typexpr` yet,
//! so each test builds the operand's type and a variable of that type directly
//! in the arenas, then constructs the index/slice node by hand.

use guff::ast::{BasicLit, Expr, Ident, IndexExpr, SliceExpr};
use guff::position::Pos;
use guff::token::Token;

use guff_types::array::new_array;
use guff_types::basic::BasicKind;
use guff_types::map::new_map;
use guff_types::object::var::new_var;
use guff_types::operand::OperandMode;
use guff_types::scope::insert as scope_insert;
use guff_types::slice::new_slice;
use guff_types::{Checker, Config, Operand, TypeId, TypeKind};

/// A fresh checker whose lookup scope is the (empty) package scope.
fn checker() -> Checker {
    let mut check = Checker::new(Config::default());
    let pkg_scope = check.packages.get(check.pkg).scope();
    check.env.scope = Some(pkg_scope);
    check
}

/// Declare a package-scope variable `name` of type `typ`.
fn add_var(check: &mut Checker, name: &str, typ: TypeId) {
    let pkg_scope = check.packages.get(check.pkg).scope();
    let v = new_var(&mut check.objects, name, typ);
    scope_insert(&mut check.scopes, &mut check.objects, pkg_scope, v);
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

fn str_lit(v: &str) -> Expr {
    Expr::BasicLit(BasicLit {
        id: 0,
        value_pos: Pos(1),
        value_end: Pos(1),
        kind: Some(Token::STRING),
        value: format!("\"{}\"", v),
    })
}

fn index_expr(base: &str, index: Expr) -> IndexExpr {
    IndexExpr {
        id: 0,
        x: Box::new(ident(base)),
        lbrack: Pos(1),
        index: Box::new(index),
        rbrack: Pos(1),
    }
}

#[test]
fn slice_index() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let slice_t = new_slice(&mut check.types, int_t);
    add_var(&mut check, "s", slice_t);

    let e = index_expr("s", int_lit("0"));
    let mut op = Operand::invalid();
    check.index_expr(&mut op, &e);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Variable);
    assert_eq!(op.typ.unwrap(), int_t);
}

#[test]
fn array_index() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let arr_t = new_array(&mut check.types, int_t, 3);
    add_var(&mut check, "a", arr_t);

    let e = index_expr("a", int_lit("1"));
    let mut op = Operand::invalid();
    check.index_expr(&mut op, &e);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    // `a` is a variable, so a[i] is an addressable variable.
    assert_eq!(op.mode, OperandMode::Variable);
    assert_eq!(op.typ.unwrap(), int_t);
}

#[test]
fn array_index_out_of_bounds() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let arr_t = new_array(&mut check.types, int_t, 3);
    add_var(&mut check, "a", arr_t);

    let e = index_expr("a", int_lit("5"));
    let mut op = Operand::invalid();
    check.index_expr(&mut op, &e);
    assert!(
        !check.errors.is_empty(),
        "a constant index past the array length must error"
    );
}

#[test]
fn map_index() {
    let mut check = checker();
    let str_t = check.basic(BasicKind::String);
    let int_t = check.basic(BasicKind::Int);
    let map_t = new_map(&mut check.types, str_t, int_t);
    add_var(&mut check, "m", map_t);

    let e = index_expr("m", str_lit("k"));
    let mut op = Operand::invalid();
    check.index_expr(&mut op, &e);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::MapIndex);
    assert_eq!(op.typ.unwrap(), int_t);
}

#[test]
fn string_index_yields_byte() {
    let mut check = checker();
    let str_t = check.basic(BasicKind::String);
    add_var(&mut check, "s", str_t);

    let e = index_expr("s", int_lit("0"));
    let mut op = Operand::invalid();
    check.index_expr(&mut op, &e);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    // An indexed string yields a (non-constant) byte value.
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap(), check.basic(BasicKind::Uint8));
}

#[test]
fn non_integer_index() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let slice_t = new_slice(&mut check.types, int_t);
    add_var(&mut check, "s", slice_t);

    // s["x"] — a string index into a slice is not an integer.
    let e = index_expr("s", str_lit("x"));
    let mut op = Operand::invalid();
    check.index_expr(&mut op, &e);
    assert!(
        !check.errors.is_empty(),
        "a non-integer index must report an error"
    );
}

#[test]
fn index_non_indexable() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    add_var(&mut check, "x", int_t);

    let e = index_expr("x", int_lit("0"));
    let mut op = Operand::invalid();
    check.index_expr(&mut op, &e);
    assert_eq!(op.mode, OperandMode::Invalid);
    assert!(
        !check.errors.is_empty(),
        "indexing a non-indexable value must report an error"
    );
}

#[test]
fn slice_of_slice() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let slice_t = new_slice(&mut check.types, int_t);
    add_var(&mut check, "s", slice_t);

    // s[1:2]
    let e = SliceExpr {
        id: 0,
        x: Box::new(ident("s")),
        lbrack: Pos(1),
        low: Some(Box::new(int_lit("1"))),
        high: Some(Box::new(int_lit("2"))),
        max: None,
        slice3: false,
        rbrack: Pos(1),
    };
    let mut op = Operand::invalid();
    check.slice_expr(&mut op, &e);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    // Slicing a slice yields the same slice type.
    assert_eq!(op.typ.unwrap().kind(&check.types), TypeKind::Slice);
}

#[test]
fn slice_swapped_indices() {
    let mut check = checker();
    let int_t = check.basic(BasicKind::Int);
    let slice_t = new_slice(&mut check.types, int_t);
    add_var(&mut check, "s", slice_t);

    // s[2:1] — high < low for constant indices is an error.
    let e = SliceExpr {
        id: 0,
        x: Box::new(ident("s")),
        lbrack: Pos(1),
        low: Some(Box::new(int_lit("2"))),
        high: Some(Box::new(int_lit("1"))),
        max: None,
        slice3: false,
        rbrack: Pos(1),
    };
    let mut op = Operand::invalid();
    check.slice_expr(&mut op, &e);
    assert!(
        !check.errors.is_empty(),
        "swapped constant slice indices must report an error"
    );
}
