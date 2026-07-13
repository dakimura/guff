//! Internal helpers shared across the public modules.
//!
//! These mirror the unexported helpers in go/constant/value.go:
//! `makeInt`, `makeRat`, `makeFloat`, `makeComplex`, `smallInt`, `smallFloat`,
//! `i64toi`, `i64tor`, `i64tof`, `itor`, `itof`, `rtof`, `vtoc`, plus the
//! ordering/matching used by `BinaryOp` and `Compare` to coerce operands to a
//! common representation.

use dashu::base::{BitTest, Sign};
use dashu::float::round::mode::HalfEven;
use dashu::float::FBig;
use dashu::integer::IBig;
use dashu::rational::RBig;

use crate::value::{BinFloat, Value, PREC};

/// Mirror of `maxExp` in Go's value.go — used to decide when a [`BinFloat`] /
/// [`RBig`] is small enough to keep in its current representation.
pub(crate) const MAX_EXP: i32 = 4 << 10;

// ----------------------------------------------------------------------------
// Variant choosers

/// Wrap an arbitrary-precision integer in the most compact [`Value`] form: if
/// it fits in an `i64`, return [`Value::Int64`]; otherwise [`Value::Int`].
///
/// Mirrors `makeInt` in Go's value.go.
pub(crate) fn make_int(x: IBig) -> Value {
    match i64::try_from(&x) {
        Ok(v) => Value::Int64(v),
        Err(_) => Value::Int(x),
    }
}

/// Wrap a rational as a [`Value::Rat`], or promote to [`Value::Float`] if
/// numerator/denominator are too large to keep in fraction form.
///
/// Mirrors `makeRat` in Go's value.go. Note: Go intentionally keeps `n/1` as
/// `ratVal{n/1}` (kind = Float) rather than collapsing to an `Int`; this lets
/// the caller distinguish "1.0" from "1" by `Kind` alone. We mirror that.
pub(crate) fn make_rat(r: RBig) -> Value {
    let num_small = small_int(r.numerator());
    let den_small = r.denominator().bit_len() < MAX_EXP as usize;
    if num_small && den_small {
        return Value::Rat(r);
    }
    Value::Float(rbig_to_fbig(r))
}

/// Wrap a [`BinFloat`] in [`Value::Float`].
///
/// Mirrors `makeFloat` in Go's value.go. Go's helper also returns
/// `Value::Unknown` for `±Inf`; we mirror that behavior.
pub(crate) fn make_float(f: BinFloat) -> Value {
    if !is_finite(&f) {
        return Value::Unknown;
    }
    Value::Float(f)
}

/// Construct a [`Value::Complex`] from real and imaginary parts.
///
/// Mirrors `makeComplex` in Go's value.go. If either part is [`Value::Unknown`]
/// the entire complex value is `Unknown`.
pub(crate) fn make_complex(re: Value, im: Value) -> Value {
    if matches!(re, Value::Unknown) || matches!(im, Value::Unknown) {
        return Value::Unknown;
    }
    Value::Complex {
        re: Box::new(re),
        im: Box::new(im),
    }
}

// ----------------------------------------------------------------------------
// "Smallness" predicates

/// Mirror of `smallInt` in Go: an integer is "small" when its magnitude does
/// not exceed `MAX_EXP` bits — i.e. it could plausibly fit in a float
/// representation without absurd memory cost.
pub(crate) fn small_int(x: &IBig) -> bool {
    x.bit_len() < MAX_EXP as usize
}

/// Mirror of `smallFloat` in Go: a float is "small" when its base-2 exponent
/// stays within `±MAX_EXP`. Operations that would convert this float to a
/// rational/integer use this to avoid producing huge representations.
pub(crate) fn small_float(f: &BinFloat) -> bool {
    if is_zero(f) {
        return true;
    }
    let exp = base2_exponent(f);
    -MAX_EXP < exp && exp < MAX_EXP
}

/// Returns true when `f` represents zero (positive or negative).
pub(crate) fn is_zero(f: &BinFloat) -> bool {
    f.repr().is_zero()
}

/// Returns true when `f` is finite (not `±Inf`).
///
/// `FBig` cannot represent NaN by construction. Infinities are represented by
/// a zero significand with a non-zero exponent; we treat those as non-finite.
pub(crate) fn is_finite(f: &BinFloat) -> bool {
    !f.repr().is_infinite()
}

/// Approximate base-2 exponent of a (non-zero) [`BinFloat`]. The mantissa is
/// stored as a signed `IBig` significand `m` plus an exponent `e` such that the
/// value is `m * 2^e`; the magnitude exponent is `e + bit_len(|m|) - 1`.
pub(crate) fn base2_exponent(f: &BinFloat) -> i32 {
    let repr = f.repr();
    let exp = repr.exponent();
    let bits = repr.significand().bit_len();
    let mag_bits = if bits == 0 { 0 } else { bits as isize - 1 };
    (exp + mag_bits) as i32
}

// ----------------------------------------------------------------------------
// Inter-variant conversions (mirror Go's i64toi, i64tor, i64tof, itor, itof,
// rtof, vtoc helpers).

/// `i64` → arbitrary-precision integer.
pub(crate) fn i64_to_ibig(x: i64) -> IBig {
    IBig::from(x)
}

/// `i64` → exact rational.
pub(crate) fn i64_to_rbig(x: i64) -> RBig {
    RBig::from(IBig::from(x))
}

/// `i64` → binary float at [`PREC`] precision.
pub(crate) fn i64_to_fbig(x: i64) -> BinFloat {
    ibig_to_fbig(IBig::from(x))
}

/// `IBig` → exact rational.
pub(crate) fn ibig_to_rbig(x: IBig) -> RBig {
    RBig::from(x)
}

/// `IBig` → binary float at [`PREC`] precision.
pub(crate) fn ibig_to_fbig(x: IBig) -> BinFloat {
    let f = FBig::<HalfEven, 2>::from_parts(x, 0);
    f.with_precision(PREC).value()
}

/// `RBig` → binary float at [`PREC`] precision.
pub(crate) fn rbig_to_fbig(r: RBig) -> BinFloat {
    let (num, den) = r.into_parts();
    let num_f: BinFloat = ibig_to_fbig(num);
    let den_f: BinFloat = ibig_to_fbig(IBig::from(den));
    (num_f / den_f).with_precision(PREC).value()
}

/// Numeric `Value` → [`Value::Complex`] (with zero imaginary part).
///
/// Mirror of `vtoc` in Go's value.go.
pub(crate) fn v_to_complex(v: Value) -> Value {
    Value::Complex {
        re: Box::new(v),
        im: Box::new(Value::Int64(0)),
    }
}

// ----------------------------------------------------------------------------
// Ordering used by `match`/`match0` to coerce two operands to a common type.

/// Mirror of `ord` in Go: maps a `Value` variant to its complexity rank, so
/// `match0` can promote the lower-ranked operand up to the higher-ranked one.
pub(crate) fn ord(v: &Value) -> i32 {
    match v {
        Value::Unknown => 0,
        Value::Bool(_) | Value::String(_) => 1,
        Value::Int64(_) => 2,
        Value::Int(_) => 3,
        Value::Rat(_) => 4,
        Value::Float(_) => 5,
        Value::Complex { .. } => 6,
    }
}

/// Match two operands by promoting the lower-ranked one. If one is `Unknown`
/// or non-numeric, both returned values are that "x position" operand so
/// callers can panic with a proper message.
///
/// Mirror of `match` in Go's value.go.
pub(crate) fn match_pair(x: Value, y: Value) -> (Value, Value) {
    let ox = ord(&x);
    let oy = ord(&y);
    if ox < oy {
        match0(x, y)
    } else if ox > oy {
        let (ny, nx) = match0(y, x);
        (nx, ny)
    } else {
        (x, y)
    }
}

/// Helper for [`match_pair`]: invariant `ord(x) < ord(y)`. Returns
/// `(promoted_x, y)`.
fn match0(x: Value, y: Value) -> (Value, Value) {
    match &y {
        Value::Int(_) => {
            if let Value::Int64(v) = x {
                return (Value::Int(i64_to_ibig(v)), y);
            }
        }
        Value::Rat(_) => match x {
            Value::Int64(v) => return (Value::Rat(i64_to_rbig(v)), y),
            Value::Int(v) => return (Value::Rat(ibig_to_rbig(v)), y),
            ref _other => {}
        },
        Value::Float(_) => match x {
            Value::Int64(v) => return (Value::Float(i64_to_fbig(v)), y),
            Value::Int(v) => return (Value::Float(ibig_to_fbig(v)), y),
            Value::Rat(v) => return (Value::Float(rbig_to_fbig(v)), y),
            ref _other => {}
        },
        Value::Complex { .. } => match &x {
            Value::Int64(_) | Value::Int(_) | Value::Rat(_) | Value::Float(_) => {
                return (v_to_complex(x), y);
            }
            _ => {}
        },
        _ => {}
    }
    // Fallback: force unknown/invalid into "x position".
    (x.clone(), x)
}

// ----------------------------------------------------------------------------
// Sign helpers

/// Return -1, 0, or 1 for an [`IBig`]'s sign.
pub(crate) fn ibig_signum(x: &IBig) -> i32 {
    if x.is_zero() {
        0
    } else {
        match x.sign() {
            Sign::Negative => -1,
            Sign::Positive => 1,
        }
    }
}

/// Return -1, 0, or 1 for an [`RBig`]'s sign.
pub(crate) fn rbig_signum(x: &RBig) -> i32 {
    ibig_signum(x.numerator())
}

/// Return -1, 0, or 1 for a [`BinFloat`]'s sign.
pub(crate) fn fbig_signum(x: &BinFloat) -> i32 {
    if is_zero(x) {
        return 0;
    }
    match x.repr().sign() {
        Sign::Negative => -1,
        Sign::Positive => 1,
    }
}
