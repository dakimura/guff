//! guff-constant — a Rust port of Go's `go/constant` package.
//!
//! Provides untyped Go constants ([`Value`]) and operations on them
//! (arithmetic, comparison, shifts, conversions, literal parsing).
//!
//! Layout:
//! - [`value`] — the `Value` enum, `Kind`, factories, accessors, `Display`.
//! - [`ops`] — `BinaryOp`, `UnaryOp`, `Compare`, `Shift`.
//! - [`convert`] — `ToInt`, `ToFloat`, `ToComplex`, `Num`, `Denom`,
//!   `MakeImag`, `Real`, `Imag`, `MakeFromBytes`, `Bytes`.
//! - [`literal`] — `MakeFromLiteral` and helpers for parsing literal strings.

mod helpers;

pub mod convert;
pub mod literal;
pub mod ops;
pub mod utf8;
pub mod value;

pub use convert::{
    bytes, denom, imag, make_from_bytes, make_imag, num, real, to_complex, to_float, to_int,
};
pub use literal::make_from_literal;
pub use utf8::decode_lossy;
pub use ops::{binary_op, compare, shift, unary_op};
pub use value::{
    bit_len, bool_val, float32_val, float64_val, int64_val, make, make_bool, make_float64,
    make_int64, make_string, make_string_bytes, make_uint64, make_unknown, sign, string_val,
    string_val_lossy, uint64_val, val, BinFloat, Kind, ValRepr, Value, PREC,
};
