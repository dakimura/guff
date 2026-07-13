//! Integration tests for `BinaryOp`, `UnaryOp`, `Shift`, `Compare`.
//!
//! These mirror selected cases from go/constant/value_test.go without yet
//! relying on `MakeFromLiteral` (which has not been ported).

use guff::token::Token;
use guff_constant::{
    binary_op, bit_len, bool_val, compare, int64_val, make_bool, make_float64, make_imag,
    make_int64, make_string, make_uint64, make_unknown, shift, sign, string_val, unary_op, Kind,
    Value,
};

// ---- Unary ----

#[test]
fn unary_plus_is_identity() {
    let v = make_int64(7);
    let r = unary_op(Token::ADD, v.clone(), 0);
    assert_eq!(int64_val(&r), (7, true));
}

#[test]
fn unary_minus_negates() {
    let v = make_int64(7);
    let r = unary_op(Token::SUB, v, 0);
    assert_eq!(int64_val(&r), (-7, true));
}

#[test]
fn unary_minus_i64_min_overflows_to_ibig() {
    // -i64::MIN does not fit in i64.
    let v = make_int64(i64::MIN);
    let r = unary_op(Token::SUB, v, 0);
    assert_eq!(r.kind(), Kind::Int);
    // Read-back should report inexact because it now lives in the IBig variant.
    let (_, exact) = int64_val(&r);
    assert!(!exact);
}

#[test]
fn unary_not_negates_bool() {
    let r = unary_op(Token::NOT, make_bool(true), 0);
    assert_eq!(bool_val(&r), false);
    let r = unary_op(Token::NOT, make_bool(false), 0);
    assert_eq!(bool_val(&r), true);
}

#[test]
fn unary_xor_bitwise_complement() {
    // ^5 == -6 (two's complement on arbitrary-precision int).
    let r = unary_op(Token::XOR, make_int64(5), 0);
    assert_eq!(int64_val(&r), (-6, true));
}

#[test]
fn unary_xor_with_prec_masks_to_unsigned() {
    // ^0 with 8-bit prec == 0xff.
    let r = unary_op(Token::XOR, make_int64(0), 8);
    assert_eq!(int64_val(&r), (0xff, true));
}

#[test]
fn unary_on_unknown_stays_unknown() {
    for op in [Token::ADD, Token::SUB, Token::XOR, Token::NOT] {
        assert_eq!(unary_op(op, make_unknown(), 0).kind(), Kind::Unknown);
    }
}

// ---- Binary arithmetic on int64 ----

#[test]
fn add_sub_mul_on_int64_stay_on_fast_path() {
    let r = binary_op(make_int64(3), Token::ADD, make_int64(4));
    assert_eq!(int64_val(&r), (7, true));
    let r = binary_op(make_int64(10), Token::SUB, make_int64(3));
    assert_eq!(int64_val(&r), (7, true));
    let r = binary_op(make_int64(6), Token::MUL, make_int64(7));
    assert_eq!(int64_val(&r), (42, true));
}

#[test]
fn add_int64_overflow_promotes_to_ibig() {
    // i64::MAX + 1 — i64::MAX has bit_len 63, so is_63bit fails.
    let r = binary_op(make_int64(i64::MAX), Token::ADD, make_int64(1));
    assert_eq!(r.kind(), Kind::Int);
    let (_, exact) = int64_val(&r);
    assert!(!exact, "expected promotion to IBig (inexact i64 read-back)");
}

#[test]
fn quo_of_ints_yields_rat() {
    // 3 / 2 = 3/2 — division of ints in go/constant produces a Float (rational).
    let r = binary_op(make_int64(3), Token::QUO, make_int64(2));
    assert_eq!(r.kind(), Kind::Float);
}

#[test]
fn quo_assign_forces_integer_division() {
    // 7 /= 2 = 3
    let r = binary_op(make_int64(7), Token::QuoAssign, make_int64(2));
    assert_eq!(r.kind(), Kind::Int);
    assert_eq!(int64_val(&r), (3, true));
}

#[test]
fn rem_int64() {
    let r = binary_op(make_int64(7), Token::REM, make_int64(3));
    assert_eq!(int64_val(&r), (1, true));
}

// ---- Bitwise on int64 ----

#[test]
fn bitwise_int64() {
    let r = binary_op(make_int64(0b1100), Token::AND, make_int64(0b1010));
    assert_eq!(int64_val(&r).0, 0b1000);
    let r = binary_op(make_int64(0b1100), Token::OR, make_int64(0b1010));
    assert_eq!(int64_val(&r).0, 0b1110);
    let r = binary_op(make_int64(0b1100), Token::XOR, make_int64(0b1010));
    assert_eq!(int64_val(&r).0, 0b0110);
    // 0b1100 &^ 0b1010 == 0b0100 (clear bits set in y)
    let r = binary_op(make_int64(0b1100), Token::AndNot, make_int64(0b1010));
    assert_eq!(int64_val(&r).0, 0b0100);
}

// ---- Mixed-type promotion ----

#[test]
fn int_plus_float_promotes_to_rat() {
    // int64(1) + float(0.5) should be a Float (specifically Rat == 3/2).
    let r = binary_op(make_int64(1), Token::ADD, make_float64(0.5));
    assert_eq!(r.kind(), Kind::Float);
}

#[test]
fn unknown_taints_binary_op() {
    let r = binary_op(make_unknown(), Token::ADD, make_int64(1));
    assert_eq!(r.kind(), Kind::Unknown);
    let r = binary_op(make_int64(1), Token::ADD, make_unknown());
    assert_eq!(r.kind(), Kind::Unknown);
}

// ---- Strings ----

#[test]
fn string_concat() {
    let r = binary_op(make_string("foo"), Token::ADD, make_string("bar"));
    assert_eq!(r.kind(), Kind::String);
    assert_eq!(string_val(&r), "foobar");
}

// ---- Complex ----

#[test]
fn complex_addition() {
    // (1 + 2i) + (3 + 4i) = (4 + 6i)
    let a = binary_op(make_int64(1), Token::ADD, make_imag(make_int64(2)));
    let b = binary_op(make_int64(3), Token::ADD, make_imag(make_int64(4)));
    let r = binary_op(a, Token::ADD, b);
    assert_eq!(r.kind(), Kind::Complex);
    if let Value::Complex { re, im } = r {
        assert_eq!(int64_val(&re), (4, true));
        assert_eq!(int64_val(&im), (6, true));
    } else {
        panic!("expected Complex");
    }
}

#[test]
fn complex_multiplication() {
    // (1 + 2i) * (3 + 4i) = (1*3 - 2*4) + i(2*3 + 1*4) = -5 + 10i
    let a = binary_op(make_int64(1), Token::ADD, make_imag(make_int64(2)));
    let b = binary_op(make_int64(3), Token::ADD, make_imag(make_int64(4)));
    let r = binary_op(a, Token::MUL, b);
    if let Value::Complex { re, im } = r {
        assert_eq!(int64_val(&re), (-5, true));
        assert_eq!(int64_val(&im), (10, true));
    } else {
        panic!("expected Complex");
    }
}

// ---- Comparisons ----

#[test]
fn compare_int_relations() {
    assert!(compare(make_int64(3), Token::LSS, make_int64(4)));
    assert!(compare(make_int64(4), Token::LEQ, make_int64(4)));
    assert!(compare(make_int64(5), Token::GTR, make_int64(4)));
    assert!(compare(make_int64(5), Token::GEQ, make_int64(5)));
    assert!(compare(make_int64(5), Token::EQL, make_int64(5)));
    assert!(compare(make_int64(5), Token::NEQ, make_int64(4)));
}

#[test]
fn compare_mixed_int_and_uint() {
    // int(1) < uint(2^63) — both Int-kind, latter is in the IBig path.
    let big = make_uint64(u64::MAX);
    assert!(compare(make_int64(1), Token::LSS, big));
}

#[test]
fn compare_unknown_is_false() {
    assert!(!compare(make_unknown(), Token::EQL, make_int64(1)));
    assert!(!compare(make_int64(1), Token::EQL, make_unknown()));
}

#[test]
fn compare_strings_use_lexicographic_order() {
    assert!(compare(make_string("abc"), Token::LSS, make_string("abd")));
    assert!(compare(make_string("abc"), Token::EQL, make_string("abc")));
}

// ---- Shifts ----

#[test]
fn shift_left_int64() {
    let r = shift(make_int64(1), Token::SHL, 3);
    assert_eq!(int64_val(&r), (8, true));
}

#[test]
fn shift_right_int64() {
    let r = shift(make_int64(64), Token::SHR, 3);
    assert_eq!(int64_val(&r), (8, true));
}

#[test]
fn shift_zero_returns_self() {
    let r = shift(make_int64(7), Token::SHL, 0);
    assert_eq!(int64_val(&r), (7, true));
}

#[test]
fn shift_left_promotes_to_ibig() {
    // 1 << 100 doesn't fit in i64.
    let r = shift(make_int64(1), Token::SHL, 100);
    assert_eq!(r.kind(), Kind::Int);
    assert_eq!(bit_len(&r), 101);
}

#[test]
fn shift_unknown_stays_unknown() {
    assert_eq!(
        shift(make_unknown(), Token::SHL, 5).kind(),
        Kind::Unknown
    );
}

// ---- Sign / BitLen ----

#[test]
fn sign_basics() {
    assert_eq!(sign(&make_int64(-3)), -1);
    assert_eq!(sign(&make_int64(0)), 0);
    assert_eq!(sign(&make_int64(3)), 1);
    // Unknown returns 1 (per spec, to suppress div-by-zero spurious errors).
    assert_eq!(sign(&make_unknown()), 1);
}

#[test]
fn bit_len_basics() {
    assert_eq!(bit_len(&make_int64(0)), 0);
    assert_eq!(bit_len(&make_int64(1)), 1);
    assert_eq!(bit_len(&make_int64(255)), 8);
    assert_eq!(bit_len(&make_int64(-1)), 1);
    assert_eq!(bit_len(&make_unknown()), 0);
}
