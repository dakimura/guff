//! Chunk-14 tests: `operand.rs` — Operand struct, modes, set_const,
//! composite_kind, operand_string stub.

use guff::token::Token;
use guff_constant::Value;
use guff_types::{
    composite_kind, init_universe_full, new_chan, new_interface_type, new_map, new_named,
    new_pointer, new_signature_type, new_slice, new_struct, new_type_name, operand_string,
    BasicKind, ChanDir, Operand, OperandMode,
};

// ----------------------------------------------------------------------------
// Defaults

#[test]
fn default_operand_is_invalid() {
    let x = Operand::invalid();
    assert_eq!(x.mode, OperandMode::Invalid);
    assert!(x.expr.is_none());
    assert!(x.typ.is_none());
    assert!(x.val.is_none());
    assert!(x.id.is_none());
}

#[test]
fn default_via_default_trait() {
    let x: Operand = Operand::default();
    assert_eq!(x.mode, OperandMode::Invalid);
}

#[test]
fn pos_returns_zero_when_no_expr() {
    let x = Operand::invalid();
    assert_eq!(x.pos(), 0);
}

#[test]
fn is_nil_only_true_for_nilvalue_mode() {
    let mut x = Operand::invalid();
    assert!(!x.is_nil());
    x.mode = OperandMode::Value;
    assert!(!x.is_nil());
    x.mode = OperandMode::NilValue;
    assert!(x.is_nil());
}

// ----------------------------------------------------------------------------
// set_const

#[test]
fn set_const_int_lit_produces_untyped_int_constant() {
    let u = init_universe_full();
    let mut op = Operand::invalid();
    op.set_const(&u.typ, Token::INT, "42");
    assert_eq!(op.mode, OperandMode::Constant);
    assert_eq!(op.typ, Some(u.typ[BasicKind::UntypedInt as usize]));
    match op.val {
        Some(Value::Int64(v)) => assert_eq!(v, 42),
        other => panic!("expected Int64, got {:?}", other),
    }
}

#[test]
fn set_const_float_lit_produces_untyped_float_constant() {
    let u = init_universe_full();
    let mut op = Operand::invalid();
    op.set_const(&u.typ, Token::FLOAT, "3.5");
    assert_eq!(op.mode, OperandMode::Constant);
    assert_eq!(op.typ, Some(u.typ[BasicKind::UntypedFloat as usize]));
    assert!(matches!(op.val, Some(_)));
}

#[test]
fn set_const_string_lit_produces_untyped_string_constant() {
    let u = init_universe_full();
    let mut op = Operand::invalid();
    op.set_const(&u.typ, Token::STRING, "\"hello\"");
    assert_eq!(op.mode, OperandMode::Constant);
    assert_eq!(op.typ, Some(u.typ[BasicKind::UntypedString as usize]));
    match op.val {
        Some(Value::String(ref s)) => assert_eq!(s.as_str(), "hello"),
        other => panic!("expected String, got {:?}", other),
    }
}

#[test]
fn set_const_invalid_literal_makes_invalid_operand() {
    let u = init_universe_full();
    let mut op = Operand::invalid();
    // "abc" is not a valid integer literal.
    op.set_const(&u.typ, Token::INT, "abc");
    assert_eq!(op.mode, OperandMode::Invalid);
    assert_eq!(op.typ, Some(u.typ[BasicKind::Invalid as usize]));
}

#[test]
fn set_const_rune_lit() {
    let u = init_universe_full();
    let mut op = Operand::invalid();
    op.set_const(&u.typ, Token::CHAR, "'A'");
    assert_eq!(op.mode, OperandMode::Constant);
    assert_eq!(op.typ, Some(u.typ[BasicKind::UntypedRune as usize]));
}

#[test]
#[should_panic(expected = "not a literal token")]
fn set_const_with_non_literal_token_panics() {
    let u = init_universe_full();
    let mut op = Operand::invalid();
    op.set_const(&u.typ, Token::IDENT, "x");
}

// ----------------------------------------------------------------------------
// composite_kind

#[test]
fn composite_kind_for_each_composite_type() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];

    let s = new_slice(&mut u.type_arena, int);
    assert_eq!(composite_kind(&u.type_arena, s), "slice");

    let m = new_map(&mut u.type_arena, int, int);
    assert_eq!(composite_kind(&u.type_arena, m), "map");

    let c = new_chan(&mut u.type_arena, ChanDir::SendRecv, int);
    assert_eq!(composite_kind(&u.type_arena, c), "chan");

    let p = new_pointer(&mut u.type_arena, int);
    assert_eq!(composite_kind(&u.type_arena, p), "pointer");

    let stt = new_struct(&mut u.type_arena, vec![], vec![]);
    assert_eq!(composite_kind(&u.type_arena, stt), "struct");

    let sig = new_signature_type(&mut u.type_arena, None, &[], &[], None, None, false);
    assert_eq!(composite_kind(&u.type_arena, sig), "func");

    let iface = new_interface_type(&mut u.type_arena, vec![], vec![]);
    assert_eq!(composite_kind(&u.type_arena, iface), "interface");
}

#[test]
fn composite_kind_for_basic_is_empty() {
    let u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    assert_eq!(composite_kind(&u.type_arena, int), "");
}

#[test]
fn composite_kind_walks_through_named() {
    // type S []int → composite_kind should follow the underlying.
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let s = new_slice(&mut u.type_arena, int);
    let tn = new_type_name(&mut u.object_arena, "S", None);
    let n = new_named(&mut u.type_arena, &mut u.object_arena, tn, Some(s), vec![]);
    assert_eq!(composite_kind(&u.type_arena, n), "slice");
}

// ----------------------------------------------------------------------------
// operand_string (stub)

#[test]
fn operand_string_for_invalid_includes_mode() {
    let u = init_universe_full();
    let x = Operand::invalid();
    let s = operand_string(&u.type_arena, &u.object_arena, &u.package_arena, &x);
    assert!(s.contains("invalid"));
}

#[test]
fn operand_string_for_nil_with_invalid_type() {
    let u = init_universe_full();
    let mut x = Operand::invalid();
    x.mode = OperandMode::NilValue;
    x.typ = Some(u.typ[BasicKind::Invalid as usize]);
    assert_eq!(
        operand_string(&u.type_arena, &u.object_arena, &u.package_arena, &x),
        "nil (with invalid type)"
    );
}

#[test]
fn operand_string_for_untyped_nil_is_just_nil() {
    let u = init_universe_full();
    let mut x = Operand::invalid();
    x.mode = OperandMode::NilValue;
    x.typ = Some(u.typ[BasicKind::UntypedNil as usize]);
    assert_eq!(
        operand_string(&u.type_arena, &u.object_arena, &u.package_arena, &x),
        "nil"
    );
}

#[test]
fn operand_string_for_constant_includes_mode_and_val() {
    let u = init_universe_full();
    let mut x = Operand::invalid();
    x.set_const(&u.typ, Token::INT, "42");
    let s = operand_string(&u.type_arena, &u.object_arena, &u.package_arena, &x);
    assert!(s.contains("constant"));
    assert!(s.contains("42"));
}
