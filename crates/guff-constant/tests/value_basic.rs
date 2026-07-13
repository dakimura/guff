//! Sanity tests for the core [`Value`] type and basic factories / accessors.
//!
//! Full behavioral parity with go/constant's value_test.go will follow once
//! BinaryOp/UnaryOp/Compare/Shift are ported.

use guff_constant::{
    bool_val, int64_val, make_bool, make_float64, make_int64, make_string, make_uint64,
    make_unknown, string_val, uint64_val, Kind, Value,
};

#[test]
fn kinds_match_constructors() {
    assert_eq!(make_unknown().kind(), Kind::Unknown);
    assert_eq!(make_bool(true).kind(), Kind::Bool);
    assert_eq!(make_string("hi").kind(), Kind::String);
    assert_eq!(make_int64(42).kind(), Kind::Int);
    assert_eq!(make_uint64(u64::MAX).kind(), Kind::Int);
    assert_eq!(make_float64(1.5).kind(), Kind::Float);
}

#[test]
fn bool_round_trip() {
    assert!(bool_val(&make_bool(true)));
    assert!(!bool_val(&make_bool(false)));
    // Unknown produces false.
    assert!(!bool_val(&make_unknown()));
}

#[test]
fn string_round_trip() {
    assert_eq!(string_val(&make_string("hello")), "hello");
    assert_eq!(string_val(&make_unknown()), "");
}

#[test]
fn int64_round_trip() {
    let (v, exact) = int64_val(&make_int64(123));
    assert_eq!(v, 123);
    assert!(exact);

    let (v, exact) = int64_val(&make_unknown());
    assert_eq!(v, 0);
    assert!(!exact);
}

#[test]
fn uint64_small_uses_int64_variant() {
    // Within i64 range we stay on the fast Int64 path.
    match make_uint64(5) {
        Value::Int64(5) => {}
        other => panic!("expected Int64(5), got {:?}", other),
    }
    let (v, exact) = uint64_val(&make_uint64(5));
    assert_eq!(v, 5);
    assert!(exact);
}

#[test]
fn uint64_large_uses_int_variant() {
    // Above i64::MAX we must promote to the IBig variant.
    let big = u64::MAX;
    match make_uint64(big) {
        Value::Int(_) => {}
        other => panic!("expected Int(IBig), got {:?}", other),
    }
    let (v, exact) = uint64_val(&make_uint64(big));
    assert_eq!(v, big);
    assert!(exact);
}

#[test]
fn int64_inexact_when_value_is_bigint() {
    // A value above i64::MAX is inexact when read back as i64.
    let v = make_uint64(u64::MAX);
    let (_, exact) = int64_val(&v);
    assert!(!exact);
}

#[test]
fn float_nonfinite_becomes_unknown() {
    assert_eq!(make_float64(f64::INFINITY).kind(), Kind::Unknown);
    assert_eq!(make_float64(f64::NEG_INFINITY).kind(), Kind::Unknown);
    assert_eq!(make_float64(f64::NAN).kind(), Kind::Unknown);
}

#[test]
fn float_small_uses_rat_variant() {
    // Small finite f64 should be stored as an exact Rat.
    match make_float64(1.5) {
        Value::Rat(_) => {}
        other => panic!("expected Rat for 1.5, got {:?}", other),
    }
}

#[test]
fn float_negative_zero_normalizes_to_zero() {
    let v = make_float64(-0.0);
    // Expect Rat(0). Either way, displaying should not yield a minus sign.
    let s = v.to_string();
    assert!(!s.starts_with('-'), "expected non-negative display, got {:?}", s);
}

#[test]
fn unknown_display_is_unknown() {
    assert_eq!(make_unknown().to_string(), "unknown");
}

#[test]
fn exact_string_quotes_strings() {
    let v = make_string("ab\"cd");
    assert_eq!(v.exact_string(), "\"ab\\\"cd\"");
}
