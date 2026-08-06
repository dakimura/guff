//! Port of go/constant/value.go numeric-conversion section:
//! `ToInt`, `ToFloat`, `ToComplex`, `Num`, `Denom`, `MakeImag`, `Real`,
//! `Imag`, `MakeFromBytes`, `Bytes`.

use dashu::base::{Gcd, UnsignedAbs};
use dashu::integer::{IBig, UBig};

use crate::helpers::{
    ibig_signum, ibig_to_fbig, ibig_to_rbig, is_finite, make_complex, make_int, small_float,
    small_int, v_to_complex,
};
use crate::value::{sign, BinFloat, Kind, Value};

// ----------------------------------------------------------------------------
// MakeImag / Real / Imag — extracting / packing complex parts.

/// Returns the complex value `x*i`.
///
/// `x` must be [`Kind::Int`], [`Kind::Float`], or [`Kind::Unknown`]. If `x` is
/// `Unknown`, the result is `Unknown`.
///
/// Equivalent to `constant.MakeImag`.
///
/// # Panics
/// Panics if `x` is neither Int nor Float (nor Unknown).
pub fn make_imag(x: Value) -> Value {
    match &x {
        Value::Unknown => x,
        Value::Int64(_) | Value::Int(_) | Value::Rat(_) | Value::Float(_) => {
            make_complex(Value::Int64(0), x)
        }
        other => panic!("{:?} not Int or Float", other),
    }
}

/// Returns the real part of `x`. `x` must be numeric or [`Kind::Unknown`].
/// If `x` is `Unknown`, the result is `Unknown`.
///
/// Equivalent to `constant.Real`.
///
/// # Panics
/// Panics if `x` is non-numeric (e.g. Bool, String).
pub fn real(x: Value) -> Value {
    match x {
        Value::Unknown
        | Value::Int64(_)
        | Value::Int(_)
        | Value::Rat(_)
        | Value::Float(_) => x,
        Value::Complex { re, .. } => *re,
        other => panic!("{:?} not numeric", other),
    }
}

/// Returns the imaginary part of `x`. `x` must be numeric or [`Kind::Unknown`].
/// If `x` is `Unknown`, the result is `Unknown`. For non-complex numerics, the
/// result is `Int64(0)`.
///
/// Equivalent to `constant.Imag`.
///
/// # Panics
/// Panics if `x` is non-numeric.
pub fn imag(x: Value) -> Value {
    match x {
        Value::Unknown => Value::Unknown,
        Value::Int64(_) | Value::Int(_) | Value::Rat(_) | Value::Float(_) => Value::Int64(0),
        Value::Complex { im, .. } => *im,
        other => panic!("{:?} not numeric", other),
    }
}

// ----------------------------------------------------------------------------
// Num / Denom — numerator and denominator views.

/// Returns the numerator of `x`. `x` must be [`Kind::Int`], [`Kind::Float`],
/// or [`Kind::Unknown`]. If `x` is `Unknown` or cannot be represented as a
/// rational, the result is `Unknown`. Otherwise an `Int`-kind value with the
/// same sign as `x`.
///
/// Equivalent to `constant.Num`.
pub fn num(x: Value) -> Value {
    match x {
        x @ (Value::Int64(_) | Value::Int(_)) => x,
        Value::Rat(r) => {
            let (n, _) = r.into_parts();
            make_int(n)
        }
        Value::Float(f) => {
            if !small_float(&f) {
                return Value::Unknown;
            }
            match fbig_to_ibig_pair(&f) {
                Some((num, _)) => make_int(num),
                None => Value::Unknown,
            }
        }
        Value::Unknown => Value::Unknown,
        other => panic!("{:?} not Int or Float", other),
    }
}

/// Returns the denominator of `x`. `x` must be [`Kind::Int`], [`Kind::Float`],
/// or [`Kind::Unknown`]. If `x` is `Unknown` or cannot be represented as a
/// rational, the result is `Unknown`. Otherwise an `Int`-kind value `>= 1`.
///
/// Equivalent to `constant.Denom`.
pub fn denom(x: Value) -> Value {
    match x {
        Value::Int64(_) | Value::Int(_) => Value::Int64(1),
        Value::Rat(r) => {
            let (_, d) = r.into_parts();
            make_int(IBig::from(d))
        }
        Value::Float(f) => {
            if !small_float(&f) {
                return Value::Unknown;
            }
            match fbig_to_ibig_pair(&f) {
                Some((_, den)) => make_int(IBig::from(den)),
                None => Value::Unknown,
            }
        }
        Value::Unknown => Value::Unknown,
        other => panic!("{:?} not Int or Float", other),
    }
}

/// Converts a finite [`BinFloat`] `f` to an `(numerator, denominator)`
/// rational representation. Returns `None` if no exact representation exists
/// (e.g. an infinity or a value with an exponent that overflows the rational).
///
/// Mirrors the `x.val.Rat(nil)` call used in Go's `Num`/`Denom`.
fn fbig_to_ibig_pair(f: &BinFloat) -> Option<(IBig, UBig)> {
    if !is_finite(f) {
        return None;
    }
    let repr = f.repr();
    let significand = repr.significand().clone();
    let exponent = repr.exponent();
    if exponent >= 0 {
        let exp = usize::try_from(exponent).ok()?;
        let num = significand << exp;
        Some((num, UBig::ONE))
    } else {
        let exp = usize::try_from(-exponent).ok()?;
        let mut den = UBig::ZERO;
        den.set_bit(exp);
        // Reduce by GCD so the pair is canonical.
        let (num, den) = reduce_pair(significand, den);
        Some((num, den))
    }
}

/// Reduce `(num, den)` by their greatest common divisor. Sign stays on the
/// numerator.
fn reduce_pair(num: IBig, den: UBig) -> (IBig, UBig) {
    let abs_num = num.clone().unsigned_abs();
    let gcd: UBig = abs_num.gcd(den.clone());
    if gcd == UBig::ONE {
        return (num, den);
    }
    let new_num = num / IBig::from(gcd.clone());
    let new_den = den / gcd;
    (new_num, new_den)
}

// ----------------------------------------------------------------------------
// ToInt / ToFloat / ToComplex — numeric kind coercions.

/// Converts `x` to an `Int`-kind value if representable; otherwise returns
/// `Unknown`.
///
/// Equivalent to `constant.ToInt`.
pub fn to_int(x: Value) -> Value {
    match x {
        x @ (Value::Int64(_) | Value::Int(_)) => x,
        Value::Rat(r) => {
            // Integer iff denominator == 1.
            if r.denominator() == &UBig::ONE {
                let (n, _) = r.into_parts();
                return make_int(n);
            }
            Value::Unknown
        }
        Value::Float(f) => {
            if !small_float(&f) {
                return Value::Unknown;
            }
            // Try exact conversion first.
            if let Some(n) = fbig_to_exact_ibig(&f) {
                return make_int(n);
            }
            // Then try a slightly relaxed precision in both rounding directions:
            // sometimes a tiny rounding error in prior computations makes the
            // value look non-integer when it should be.
            const DELTA: usize = 4;
            let relaxed = f.clone().with_precision(crate::value::PREC - DELTA).value();
            if let Some(n) = round_to_ibig(&relaxed, RoundMode::Zero) {
                return make_int(n);
            }
            if let Some(n) = round_to_ibig(&relaxed, RoundMode::Away) {
                return make_int(n);
            }
            Value::Unknown
        }
        Value::Complex { re, im } => {
            // Real iff imaginary part is zero. ToFloat handles that test for us.
            let re_f = to_float(Value::Complex { re, im });
            if re_f.kind() == Kind::Float {
                return to_int(re_f);
            }
            Value::Unknown
        }
        // `constant.ToInt` returns unknown for anything non-numeric (Bool,
        // String) rather than panicking — an array length like
        // `const n = "x"; type t [n]int` reaches here and must be reported as a
        // type error, not crash the checker.
        _ => Value::Unknown,
    }
}

/// Converts `x` to a `Float`-kind value if representable; otherwise returns
/// `Unknown`.
///
/// Equivalent to `constant.ToFloat`.
pub fn to_float(x: Value) -> Value {
    match x {
        Value::Int64(v) => Value::Rat(crate::helpers::i64_to_rbig(v)),
        Value::Int(v) => {
            if small_int(&v) {
                Value::Rat(ibig_to_rbig(v))
            } else {
                Value::Float(ibig_to_fbig(v))
            }
        }
        x @ (Value::Rat(_) | Value::Float(_)) => x,
        Value::Complex { re, im } => {
            if sign(&im) == 0 {
                return to_float(*re);
            }
            Value::Unknown
        }
        // As in `constant.ToFloat`, non-numeric values convert to unknown.
        _ => Value::Unknown,
    }
}

/// Converts `x` to a `Complex`-kind value if representable; otherwise returns
/// `Unknown`.
///
/// Equivalent to `constant.ToComplex`.
pub fn to_complex(x: Value) -> Value {
    match x {
        Value::Int64(_) | Value::Int(_) | Value::Rat(_) | Value::Float(_) => v_to_complex(x),
        x @ Value::Complex { .. } => x,
        _ => Value::Unknown,
    }
}

/// True iff `f` is an exact integer at its current precision.
fn fbig_to_exact_ibig(f: &BinFloat) -> Option<IBig> {
    let repr = f.repr();
    let exp = repr.exponent();
    if exp >= 0 {
        // Significand * 2^exp is exact.
        let e = usize::try_from(exp).ok()?;
        return Some(repr.significand().clone() << e);
    }
    // exp < 0: integer iff the low |exp| bits of the significand are zero.
    let e = (-exp) as usize;
    let mag = repr.significand().clone().unsigned_abs();
    if mag.trailing_zeros().unwrap_or(0) < e {
        return None;
    }
    Some(repr.significand().clone() >> e)
}

#[derive(Clone, Copy)]
enum RoundMode {
    /// Round toward zero (truncate).
    Zero,
    /// Round away from zero (ceil for positives, floor for negatives).
    Away,
}

/// Round `f` to an `IBig` using the given rounding mode. Returns `None` if
/// the rounded value is not exact at the current precision (i.e. the
/// rounded representation differs from the true value at the discarded bits).
fn round_to_ibig(f: &BinFloat, mode: RoundMode) -> Option<IBig> {
    let repr = f.repr();
    let exp = repr.exponent();
    if exp >= 0 {
        let e = usize::try_from(exp).ok()?;
        return Some(repr.significand().clone() << e);
    }
    let e = (-exp) as usize;
    let sig = repr.significand().clone();
    let sign_negative = ibig_signum(&sig) < 0;
    let mag = sig.clone().unsigned_abs();
    let truncated = (&mag) >> e;
    let has_remainder = mag.trailing_zeros().unwrap_or(0) < e;
    let needs_bump = has_remainder
        && matches!(mode, RoundMode::Away);
    let mut result = IBig::from(truncated);
    if needs_bump {
        result += IBig::ONE;
    }
    if sign_negative {
        result = -result;
    }
    if has_remainder && matches!(mode, RoundMode::Zero) {
        // For Zero mode, truncation is exact-enough — Go's `Int` returns
        // `big.Exact` only when there's no remainder; we mirror that.
        return None;
    }
    if has_remainder && matches!(mode, RoundMode::Away) {
        return None;
    }
    Some(result)
}

// ----------------------------------------------------------------------------
// Bytes / MakeFromBytes — little-endian byte representations of Ints.

/// Returns the bytes of `|x|` in little-endian binary representation.
///
/// `x` must be [`Kind::Int`]. Equivalent to `constant.Bytes`.
///
/// # Panics
/// Panics if `x` is not Int.
pub fn bytes(x: Value) -> Vec<u8> {
    let magnitude = match x {
        Value::Int64(v) => {
            let u = if v < 0 { v.unsigned_abs() } else { v as u64 };
            UBig::from(u)
        }
        Value::Int(v) => v.unsigned_abs(),
        other => panic!("{:?} not an Int", other),
    };
    let mut bytes: Vec<u8> = magnitude.to_le_bytes().into_vec();
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    bytes
}

/// Returns the `Int` value with the given little-endian byte representation.
/// An empty slice represents zero.
///
/// Equivalent to `constant.MakeFromBytes`.
pub fn make_from_bytes(bytes: &[u8]) -> Value {
    let magnitude = UBig::from_le_bytes(bytes);
    make_int(IBig::from(magnitude))
}

