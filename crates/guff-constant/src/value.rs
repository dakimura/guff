//! Port of go/constant/value.go.
//!
//! Original Go source:
//!   Copyright 2013 The Go Authors. All rights reserved.
//!   Use of this source code is governed by a BSD-style license.
//!
//! Representation of values mirrors the original Go package: `Int` and `Float`
//! kinds each have two variants, the "smaller" / more precise one being
//! preferred. Once a `Float` value becomes a [`Value::Float`] (arbitrary
//! precision binary), subsequent results stay `Float` — no attempt is made to
//! convert back to a [`Value::Rat`].
//!
//! - [`Value::Int64`] — `Int` representable as an `i64` (fast path).
//! - [`Value::Int`]   — `Int` outside the `i64` range, using [`IBig`].
//! - [`Value::Rat`]   — `Float` representable as an exact rational ([`RBig`]).
//! - [`Value::Float`] — `Float` requiring an arbitrary-precision binary float
//!   ([`BinFloat`]), at [`PREC`] bits of mantissa precision.

use std::fmt;
use std::sync::Arc;

use dashu::base::BitTest;
use dashu::float::round::mode::HalfEven;
use dashu::float::FBig;
use dashu::integer::IBig;
use dashu::rational::RBig;

use crate::helpers::{
    fbig_signum, ibig_signum, is_zero as fbig_is_zero, make_int as helpers_make_int, rbig_signum,
};

/// Arbitrary-precision binary float type used for [`Value::Float`].
///
/// Equivalent of Go's `*big.Float` with the default rounding mode
/// (`ToNearestEven`).
pub type BinFloat = FBig<HalfEven, 2>;

/// Maximum supported mantissa precision for [`Value::Float`].
///
/// The Go spec requires at least 256 bits; the reference implementation uses
/// 512 bits, which we mirror here.
pub const PREC: usize = 512;

/// Kind specifies the kind of value represented by a [`Value`].
///
/// Mirrors `constant.Kind` in Go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Unknown — used when a value is unknown due to an error. Operations on
    /// unknown values produce unknown values unless otherwise specified.
    Unknown,
    Bool,
    String,
    Int,
    Float,
    Complex,
}

/// Value represents the value of a Go constant.
///
/// Mirrors the `constant.Value` interface in Go. Use [`Value::kind`] to
/// discriminate. Internal variant choice (e.g. [`Value::Int64`] vs
/// [`Value::Int`]) is an implementation detail — consumers should treat both
/// as `Int`-kind.
#[derive(Debug, Clone)]
pub enum Value {
    Unknown,
    Bool(bool),
    String(Arc<String>),
    Int64(i64),
    Int(IBig),
    Rat(RBig),
    Float(BinFloat),
    Complex {
        re: Box<Value>,
        im: Box<Value>,
    },
}

impl Value {
    /// Returns the [`Kind`] of this value.
    pub fn kind(&self) -> Kind {
        match self {
            Value::Unknown => Kind::Unknown,
            Value::Bool(_) => Kind::Bool,
            Value::String(_) => Kind::String,
            Value::Int64(_) | Value::Int(_) => Kind::Int,
            Value::Rat(_) | Value::Float(_) => Kind::Float,
            Value::Complex { .. } => Kind::Complex,
        }
    }
}

// ----------------------------------------------------------------------------
// Factories

/// Returns the [`Kind::Unknown`] value.
///
/// Equivalent to `constant.MakeUnknown()`.
pub fn make_unknown() -> Value {
    Value::Unknown
}

/// Returns the [`Kind::Bool`] value for `b`.
///
/// Equivalent to `constant.MakeBool(b)`.
pub fn make_bool(b: bool) -> Value {
    Value::Bool(b)
}

/// Returns the [`Kind::String`] value for `s`.
///
/// Equivalent to `constant.MakeString(s)`. The Go implementation uses a lazy
/// concatenation tree to keep `BinaryOp(Add, ...)` of strings cheap; for now
/// we eagerly own the string and revisit if profiling demands it.
pub fn make_string<S: Into<String>>(s: S) -> Value {
    Value::String(Arc::new(s.into()))
}

/// Returns the [`Kind::Int`] value for `x`.
///
/// Equivalent to `constant.MakeInt64(x)`.
pub fn make_int64(x: i64) -> Value {
    Value::Int64(x)
}

/// Returns the [`Kind::Int`] value for `x`.
///
/// Equivalent to `constant.MakeUint64(x)`. Values that fit in an `i64` use the
/// fast [`Value::Int64`] variant.
pub fn make_uint64(x: u64) -> Value {
    if x < 1u64 << 63 {
        Value::Int64(x as i64)
    } else {
        Value::Int(IBig::from(x))
    }
}

/// Returns the [`Kind::Float`] value for `x`.
///
/// Equivalent to `constant.MakeFloat64(x)`. If `x` is `-0.0`, the result is
/// `0.0`. If `x` is not finite, the result is [`Value::Unknown`].
pub fn make_float64(x: f64) -> Value {
    if !x.is_finite() {
        return Value::Unknown;
    }
    // Normalize -0.0 to 0.0 to match Go's behavior.
    let x = x + 0.0;
    if small_float64(x) {
        // Exact representation as a rational. f64 → RBig is lossless when finite.
        if let Ok(r) = RBig::try_from(x) {
            return Value::Rat(r);
        }
        return Value::Unknown;
    }
    match BinFloat::try_from(x) {
        Ok(f) => Value::Float(f.with_precision(PREC).value()),
        Err(_) => Value::Unknown,
    }
}

/// Maximum exponent for which we still consider an `f64` "small enough" to
/// keep as an exact [`Value::Rat`] rather than promoting to [`Value::Float`].
///
/// Mirrors `maxExp` in Go's value.go.
const MAX_EXP: i32 = 4 << 10;

/// Returns true iff `x` is small enough to store as a [`Value::Rat`] without
/// blowing up rational arithmetic. Mirrors `smallFloat64` in Go's value.go.
fn small_float64(x: f64) -> bool {
    if x == 0.0 {
        return true;
    }
    let (_, exp) = frexp(x);
    -MAX_EXP < exp && exp < MAX_EXP
}

/// `frexp` decomposes a finite `f64` into a normalized fraction in `[0.5, 1)`
/// and a power of two, mirroring Go's `math.Frexp`.
fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 || !x.is_finite() {
        return (x, 0);
    }
    let bits = x.to_bits();
    let mut exp = ((bits >> 52) & 0x7ff) as i32;
    let mantissa = bits & 0x000f_ffff_ffff_ffff;
    if exp == 0 {
        // Subnormal: normalize.
        let lz = mantissa.leading_zeros() as i32 - (64 - 53);
        exp = 1 - lz;
    }
    let frac_bits = (bits & 0x800f_ffff_ffff_ffff) | (0x3fe_u64 << 52);
    (f64::from_bits(frac_bits), exp - 1022)
}

// ----------------------------------------------------------------------------
// Accessors

/// Returns the Go `bool` value of `x`, which must be a [`Kind::Bool`] or
/// [`Kind::Unknown`]. Unknown produces `false`.
///
/// # Panics
/// Panics if `x` is not Bool or Unknown.
pub fn bool_val(x: &Value) -> bool {
    match x {
        Value::Bool(b) => *b,
        Value::Unknown => false,
        other => panic!("{:?} not a Bool", other),
    }
}

/// Returns the Go `String` value of `x`, which must be a [`Kind::String`] or
/// [`Kind::Unknown`]. Unknown produces `""`.
///
/// # Panics
/// Panics if `x` is not String or Unknown.
pub fn string_val(x: &Value) -> String {
    match x {
        Value::String(s) => (**s).clone(),
        Value::Unknown => String::new(),
        other => panic!("{:?} not a String", other),
    }
}

/// Returns the Go `i64` value of `x` and whether the result is exact.
///
/// `x` must be a [`Kind::Int`] or [`Kind::Unknown`]. Unknown returns
/// `(0, false)`.
///
/// # Panics
/// Panics if `x` is not Int or Unknown.
pub fn int64_val(x: &Value) -> (i64, bool) {
    match x {
        Value::Int64(v) => (*v, true),
        Value::Int(v) => {
            // Mirrors Go: returns the low 64 bits, marked inexact.
            (i64::try_from(v).unwrap_or(0), false)
        }
        Value::Unknown => (0, false),
        other => panic!("{:?} not an Int", other),
    }
}

/// Returns the Go `u64` value of `x` and whether the result is exact.
///
/// `x` must be a [`Kind::Int`] or [`Kind::Unknown`]. Unknown returns
/// `(0, false)`.
///
/// # Panics
/// Panics if `x` is not Int or Unknown.
pub fn uint64_val(x: &Value) -> (u64, bool) {
    match x {
        Value::Int64(v) => (*v as u64, *v >= 0),
        Value::Int(v) => match u64::try_from(v) {
            Ok(u) => (u, true),
            Err(_) => (0, false),
        },
        Value::Unknown => (0, false),
        other => panic!("{:?} not an Int", other),
    }
}

/// Nearest `f32` value of `x` and whether the result is exact.
///
/// `x` must be numeric or [`Kind::Unknown`], but not [`Kind::Complex`].
/// Unknown returns `(0.0, false)`.
///
/// # Panics
/// Panics if `x` is not numeric or Unknown.
pub fn float32_val(x: &Value) -> (f32, bool) {
    match x {
        Value::Int64(v) => {
            let f = *v as f32;
            (f, (f as i64) == *v)
        }
        Value::Int(v) => approx_ibig_as_f32(v),
        Value::Rat(v) => approx_rbig_as_f32(v),
        Value::Float(v) => match v.to_f32() {
            dashu::base::Approximation::Exact(f) => (f, true),
            dashu::base::Approximation::Inexact(f, _) => (f, false),
        },
        Value::Unknown => (0.0, false),
        other => panic!("{:?} not a Float", other),
    }
}

/// Nearest `f64` value of `x` and whether the result is exact.
///
/// `x` must be numeric or [`Kind::Unknown`], but not [`Kind::Complex`].
/// Unknown returns `(0.0, false)`.
///
/// # Panics
/// Panics if `x` is not numeric or Unknown.
pub fn float64_val(x: &Value) -> (f64, bool) {
    match x {
        Value::Int64(v) => {
            let f = *v as f64;
            (f, (f as i64) == *v)
        }
        Value::Int(v) => approx_ibig_as_f64(v),
        Value::Rat(v) => approx_rbig_as_f64(v),
        Value::Float(v) => match v.to_f64() {
            dashu::base::Approximation::Exact(f) => (f, true),
            dashu::base::Approximation::Inexact(f, _) => (f, false),
        },
        Value::Unknown => (0.0, false),
        other => panic!("{:?} not a Float", other),
    }
}

fn approx_ibig_as_f32(v: &IBig) -> (f32, bool) {
    let f = FBig::<HalfEven, 2>::from(v.clone());
    match f.to_f32() {
        dashu::base::Approximation::Exact(x) => (x, true),
        dashu::base::Approximation::Inexact(x, _) => (x, false),
    }
}

fn approx_ibig_as_f64(v: &IBig) -> (f64, bool) {
    let f = FBig::<HalfEven, 2>::from(v.clone());
    match f.to_f64() {
        dashu::base::Approximation::Exact(x) => (x, true),
        dashu::base::Approximation::Inexact(x, _) => (x, false),
    }
}

fn approx_rbig_as_f32(v: &RBig) -> (f32, bool) {
    match v.to_f32() {
        dashu::base::Approximation::Exact(f) => (f, true),
        dashu::base::Approximation::Inexact(f, _) => (f, false),
    }
}

fn approx_rbig_as_f64(v: &RBig) -> (f64, bool) {
    match v.to_f64() {
        dashu::base::Approximation::Exact(f) => (f, true),
        dashu::base::Approximation::Inexact(f, _) => (f, false),
    }
}

/// Untyped "dynamic" snapshot of a [`Value`], used by [`val`] / [`make`] as a
/// Rust-friendly alternative to Go's `any`-typed `Val()` / `Make()`.
///
/// The possible variants mirror the dynamic return types documented on
/// Go's [`constant.Val`](https://pkg.go.dev/go/constant#Val).
#[derive(Debug, Clone)]
pub enum ValRepr {
    /// Returned for [`Value::Unknown`] and [`Value::Complex`] — Go uses `nil`.
    Nil,
    Bool(bool),
    String(String),
    Int64(i64),
    Int(IBig),
    Rat(RBig),
    Float(BinFloat),
}

/// Returns a Rust-typed snapshot of the underlying value. Equivalent to
/// `constant.Val()` in Go (which returns `any`).
pub fn val(x: &Value) -> ValRepr {
    match x {
        Value::Bool(b) => ValRepr::Bool(*b),
        Value::String(s) => ValRepr::String((**s).clone()),
        Value::Int64(v) => ValRepr::Int64(*v),
        Value::Int(v) => ValRepr::Int(v.clone()),
        Value::Rat(v) => ValRepr::Rat(v.clone()),
        Value::Float(v) => ValRepr::Float(v.clone()),
        Value::Unknown | Value::Complex { .. } => ValRepr::Nil,
    }
}

/// Constructs a [`Value`] from a Rust-typed snapshot. Equivalent to
/// `constant.Make()` in Go. Anything not matched yields [`Value::Unknown`].
pub fn make(x: ValRepr) -> Value {
    match x {
        ValRepr::Bool(b) => make_bool(b),
        ValRepr::String(s) => make_string(s),
        ValRepr::Int64(v) => make_int64(v),
        ValRepr::Int(v) => helpers_make_int(v),
        ValRepr::Rat(v) => crate::helpers::make_rat(v),
        ValRepr::Float(v) => crate::helpers::make_float(v),
        ValRepr::Nil => Value::Unknown,
    }
}

/// Returns -1, 0, or 1 depending on whether `x < 0`, `x == 0`, or `x > 0`.
///
/// `x` must be numeric or [`Kind::Unknown`]. For complex values the result is
/// `0` iff `x == 0`, otherwise non-zero. For [`Value::Unknown`] the result is
/// `1` (mirroring Go, which uses this to suppress spurious division-by-zero
/// errors).
///
/// # Panics
/// Panics if `x` is not numeric / Unknown.
pub fn sign(x: &Value) -> i32 {
    match x {
        Value::Int64(v) => {
            if *v < 0 {
                -1
            } else if *v > 0 {
                1
            } else {
                0
            }
        }
        Value::Int(v) => ibig_signum(v),
        Value::Rat(v) => rbig_signum(v),
        Value::Float(v) => fbig_signum(v),
        Value::Complex { re, im } => sign(re) | sign(im),
        Value::Unknown => 1,
        other => panic!("{:?} not numeric", other),
    }
}

/// Returns the number of bits required to represent `|x|` in binary.
///
/// `x` must be [`Kind::Int`] or [`Kind::Unknown`]. Unknown returns `0`.
///
/// # Panics
/// Panics if `x` is not Int or Unknown.
pub fn bit_len(x: &Value) -> usize {
    match x {
        Value::Int64(v) => {
            let u = if *v < 0 {
                // Mirror Go's "uint64(-x)" without wraparound surprises.
                (v.unsigned_abs()) as u64
            } else {
                *v as u64
            };
            64 - u.leading_zeros() as usize
        }
        Value::Int(v) => v.bit_len(),
        Value::Unknown => 0,
        other => panic!("{:?} not an Int", other),
    }
}

// ----------------------------------------------------------------------------
// Display
//
// Go exposes two display forms:
//   String        — possibly shortened, for human consumption
//   ExactString   — full precision, suitable for round-tripping numerics
//
// We map `String` onto `fmt::Display`, and provide [`Value::exact_string`] for
// the full-precision form.

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Unknown => f.write_str("unknown"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::String(s) => write!(f, "{}", quote_shortened(s)),
            Value::Int64(v) => write!(f, "{}", v),
            Value::Int(v) => write!(f, "{}", v),
            Value::Rat(v) => {
                if fbig_is_zero_rat(v) {
                    f.write_str("0")
                } else {
                    fmt::Display::fmt(v, f)
                }
            }
            Value::Float(v) => {
                if fbig_is_zero(v) {
                    f.write_str("0")
                } else {
                    fmt::Display::fmt(v, f)
                }
            }
            Value::Complex { re, im } => write!(f, "({} + {}i)", re, im),
        }
    }
}

impl Value {
    /// Returns the full-precision string form, suitable for round-tripping.
    ///
    /// Equivalent to Go's `constant.Value.ExactString()`.
    pub fn exact_string(&self) -> String {
        match self {
            Value::Unknown => self.to_string(),
            Value::Bool(_) => self.to_string(),
            Value::String(s) => quote(s),
            Value::Int64(_) | Value::Int(_) => self.to_string(),
            Value::Rat(v) => v.to_string(),
            Value::Float(v) => v.to_string(),
            Value::Complex { re, im } => format!("({} + {}i)", re.exact_string(), im.exact_string()),
        }
    }
}

fn fbig_is_zero_rat(r: &RBig) -> bool {
    use dashu::base::Sign::*;
    let num = r.numerator();
    matches!(num.sign(), Positive) && i64::try_from(num) == Ok(0)
}

/// Returns `s` wrapped in Go-style double-quote escaping.
///
/// A minimal implementation of `strconv.Quote` sufficient for displaying
/// constant values; full Unicode/printability handling can come later when we
/// port `strconv` proper.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Like [`quote`], but truncates with `...` if the quoted form exceeds
/// `MAX_LEN` runes. Matches the shortening done by Go's `stringVal.String`.
fn quote_shortened(s: &str) -> String {
    const MAX_LEN: usize = 72;
    let quoted = quote(s);
    if quoted.chars().count() <= MAX_LEN {
        return quoted;
    }
    let mut out: String = quoted.chars().take(MAX_LEN - 3).collect();
    out.push_str("...");
    out
}
