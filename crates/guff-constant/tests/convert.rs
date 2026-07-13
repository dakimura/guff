//! Integration tests for the numeric-conversion functions.

use guff::token::Token;
use guff_constant::{
    binary_op, bytes, denom, imag, int64_val, make_float64, make_from_bytes, make_imag, make_int64,
    make_unknown, num, real, to_complex, to_float, to_int, uint64_val, Kind, Value,
};

#[test]
fn to_int_passes_through_ints() {
    let r = to_int(make_int64(42));
    assert_eq!(int64_val(&r), (42, true));
}

#[test]
fn to_int_of_rat_one_half_is_unknown() {
    // 1/2 isn't representable as an Int.
    let half = binary_op(make_int64(1), Token::QUO, make_int64(2));
    assert_eq!(half.kind(), Kind::Float);
    assert_eq!(to_int(half).kind(), Kind::Unknown);
}

#[test]
fn to_int_of_rat_integer_returns_int() {
    // 6/2 = 3 — exact integer; ToInt should recover it.
    let three = binary_op(make_int64(6), Token::QUO, make_int64(2));
    let r = to_int(three);
    assert_eq!(int64_val(&r), (3, true));
}

#[test]
fn to_float_of_int_returns_float() {
    let r = to_float(make_int64(7));
    assert_eq!(r.kind(), Kind::Float);
}

#[test]
fn to_float_of_real_complex_returns_float() {
    // (3 + 0i) → 3.0
    let cplx = make_imag(make_int64(0));
    let real_three = binary_op(make_int64(3), Token::ADD, cplx);
    let r = to_float(real_three);
    assert_eq!(r.kind(), Kind::Float);
}

#[test]
fn to_float_of_nonzero_imag_is_unknown() {
    // 3 + 2i — not representable as Float.
    let cplx = binary_op(make_int64(3), Token::ADD, make_imag(make_int64(2)));
    assert_eq!(to_float(cplx).kind(), Kind::Unknown);
}

#[test]
fn to_complex_promotes_numerics() {
    let r = to_complex(make_int64(5));
    assert_eq!(r.kind(), Kind::Complex);
    if let Value::Complex { re, im } = r {
        assert_eq!(int64_val(&re), (5, true));
        assert_eq!(int64_val(&im), (0, true));
    } else {
        panic!("expected Complex");
    }
}

#[test]
fn num_denom_of_int_is_int_and_one() {
    let n = num(make_int64(7));
    let d = denom(make_int64(7));
    assert_eq!(int64_val(&n), (7, true));
    assert_eq!(int64_val(&d), (1, true));
}

#[test]
fn num_denom_of_one_half() {
    // 1/2 — Num is 1, Denom is 2.
    let half = binary_op(make_int64(1), Token::QUO, make_int64(2));
    let n = num(half.clone());
    let d = denom(half);
    assert_eq!(int64_val(&n), (1, true));
    assert_eq!(int64_val(&d), (2, true));
}

#[test]
fn num_denom_of_unknown_is_unknown() {
    assert_eq!(num(make_unknown()).kind(), Kind::Unknown);
    assert_eq!(denom(make_unknown()).kind(), Kind::Unknown);
}

#[test]
fn real_imag_of_real_value() {
    // For non-complex numerics: Real(x) == x, Imag(x) == 0.
    let r = make_int64(5);
    assert_eq!(int64_val(&real(r.clone())), (5, true));
    assert_eq!(int64_val(&imag(r)), (0, true));
}

#[test]
fn real_imag_of_complex() {
    let cplx = binary_op(make_int64(3), Token::ADD, make_imag(make_int64(4)));
    assert_eq!(int64_val(&real(cplx.clone())), (3, true));
    assert_eq!(int64_val(&imag(cplx)), (4, true));
}

#[test]
fn bytes_roundtrip_through_make_from_bytes() {
    let v = make_int64(0x0123_4567_89ab_cdef);
    let b = bytes(v);
    // little-endian: lowest byte first
    assert_eq!(b, vec![0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01]);
    let back = make_from_bytes(&b);
    assert_eq!(int64_val(&back), (0x0123_4567_89ab_cdef, true));
}

#[test]
fn bytes_of_zero_is_empty() {
    assert_eq!(bytes(make_int64(0)), Vec::<u8>::new());
}

#[test]
fn make_from_bytes_of_empty_is_zero() {
    let v = make_from_bytes(&[]);
    assert_eq!(int64_val(&v), (0, true));
}

#[test]
fn bytes_of_negative_int_uses_absolute_value() {
    // Go's Bytes works on |x|, so Bytes(-5) == Bytes(5).
    let b_pos = bytes(make_int64(5));
    let b_neg = bytes(make_int64(-5));
    assert_eq!(b_pos, b_neg);
}

#[test]
fn to_float_preserves_uint_max() {
    // u64::MAX is in the IBig variant; ToFloat should keep it as a Float.
    use guff_constant::make_uint64;
    let v = make_uint64(u64::MAX);
    assert_eq!(uint64_val(&v).0, u64::MAX);
    let f = to_float(v);
    assert_eq!(f.kind(), Kind::Float);
}

#[test]
fn to_int_of_small_float_works() {
    // 3.0 → 3 via ToInt.
    let v = make_float64(3.0);
    let r = to_int(v);
    assert_eq!(int64_val(&r), (3, true));
}

#[test]
fn to_int_of_three_point_five_is_unknown() {
    let r = to_int(make_float64(3.5));
    assert_eq!(r.kind(), Kind::Unknown);
}
