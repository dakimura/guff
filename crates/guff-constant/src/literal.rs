//! Port of `constant.MakeFromLiteral` and its helpers.
//!
//! Parses a Go literal string (as produced by the lexer / `go/scanner`) into
//! a [`Value`]. The literal-kind tag (`tok`) is one of [`Token::INT`],
//! [`Token::FLOAT`], [`Token::IMAG`], [`Token::CHAR`], or [`Token::STRING`].
//!
//! The Go implementation defers parsing to the `strconv` package; here we
//! hand-roll the parts of `strconv.ParseInt`, `strconv.Unquote`, and float
//! parsing that are relevant to Go literal syntax. Invalid input produces
//! [`Value::Unknown`], matching Go's behavior.

use std::str::FromStr;

use dashu::integer::{IBig, UBig};
use dashu::rational::RBig;
use guff::token::Token;

use crate::helpers::{ibig_to_fbig, make_complex, make_int, make_rat, rbig_to_fbig, small_int};
use crate::value::{make_int64, make_string_bytes, BinFloat, Value, PREC};

/// Parses a Go literal string into a constant [`Value`].
///
/// `tok` selects the literal kind. The trailing `_zero` argument exists for
/// API parity with Go's `MakeFromLiteral(lit, tok, zero uint)` — Go panics if
/// it's non-zero, and we mirror that.
///
/// Returns [`Value::Unknown`] when the literal string is syntactically
/// invalid for the given kind.
///
/// # Panics
/// Panics when `_zero` is non-zero (Go API parity), or when `tok` is not one
/// of the valid literal token kinds.
pub fn make_from_literal(lit: &str, tok: Token, _zero: u32) -> Value {
    assert!(_zero == 0, "make_from_literal called with non-zero last argument");
    match tok {
        Token::INT => parse_int_lit(lit).unwrap_or(Value::Unknown),
        Token::FLOAT => parse_float_lit(lit).unwrap_or(Value::Unknown),
        Token::IMAG => {
            // Strip trailing 'i' and parse as float.
            if let Some(stripped) = lit.strip_suffix('i') {
                if let Some(im) = parse_float_lit(stripped) {
                    return make_complex(make_int64(0), im);
                }
            }
            Value::Unknown
        }
        Token::CHAR => parse_char_lit(lit).unwrap_or(Value::Unknown),
        Token::STRING => parse_string_lit(lit).unwrap_or(Value::Unknown),
        other => panic!("{} is not a valid literal token", other.as_str()),
    }
}

// ----------------------------------------------------------------------------
// Integer literals

fn parse_int_lit(lit: &str) -> Option<Value> {
    let (sign_negative, rest) = strip_sign(lit);
    let (radix, digits) = detect_int_radix(rest);
    let cleaned = strip_underscores(digits)?;
    if cleaned.is_empty() {
        return None;
    }
    let mag = UBig::from_str_radix(&cleaned, radix).ok()?;
    let mut value = IBig::from(mag);
    if sign_negative {
        value = -value;
    }
    // Try the i64 fast path first.
    if let Ok(v) = i64::try_from(&value) {
        return Some(make_int64(v));
    }
    Some(make_int(value))
}

fn strip_sign(s: &str) -> (bool, &str) {
    if let Some(rest) = s.strip_prefix('-') {
        return (true, rest);
    }
    if let Some(rest) = s.strip_prefix('+') {
        return (false, rest);
    }
    (false, s)
}

/// Detect a Go integer literal's base from its prefix and return the digit
/// substring with the prefix removed. Recognized prefixes:
///   `0b` / `0B` → 2,  `0o` / `0O` → 8,  `0x` / `0X` → 16,
///   leading `0` (no other prefix) with only digit chars → legacy octal,
///   anything else → 10.
fn detect_int_radix(s: &str) -> (u32, &str) {
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return (16, rest);
    }
    if let Some(rest) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        return (8, rest);
    }
    if let Some(rest) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        return (2, rest);
    }
    // Legacy Go octal: a leading 0 (with more digits, all 0-7) means octal.
    if s.len() > 1 && s.starts_with('0') && s[1..].chars().all(|c| ('0'..='7').contains(&c) || c == '_') {
        return (8, &s[1..]);
    }
    (10, s)
}

/// Remove ASCII underscores from a digit string. Underscores can only appear
/// between digits — we return `None` if they appear at the start/end or
/// adjacent to another underscore. (Go's `strconv.ParseInt` requires the same.)
fn strip_underscores(s: &str) -> Option<String> {
    if s.starts_with('_') || s.ends_with('_') || s.contains("__") {
        return None;
    }
    Some(s.chars().filter(|c| *c != '_').collect())
}

// ----------------------------------------------------------------------------
// Float literals

fn parse_float_lit(lit: &str) -> Option<Value> {
    // Empty isn't a valid Go float literal.
    if lit.is_empty() {
        return None;
    }
    let (sign_negative, rest) = strip_sign(lit);
    // Hex float?
    let parsed = if let Some(after_prefix) = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))
    {
        parse_hex_float(after_prefix)?
    } else {
        parse_decimal_float(rest)?
    };
    Some(apply_sign(parsed, sign_negative))
}

/// Apply a leading minus sign by negating the numeric `Value`.
fn apply_sign(v: Value, negative: bool) -> Value {
    if !negative {
        return v;
    }
    match v {
        Value::Int64(n) => match n.checked_neg() {
            Some(m) => Value::Int64(m),
            None => make_int(-IBig::from(n)),
        },
        Value::Int(n) => make_int(-n),
        Value::Rat(r) => make_rat(-r),
        Value::Float(f) => Value::Float(f.with_precision(PREC).value().neg()),
        other => other,
    }
}

/// Parse a Go decimal float literal: `[digits][.digits][eE[+-]?digits]`.
/// At least one digit must appear, and at least one of `.` or exponent if no
/// fractional digits.
fn parse_decimal_float(s: &str) -> Option<Value> {
    let (mantissa, exp) = split_exponent(s, &['e', 'E'])?;
    let exp: i64 = if exp.is_empty() {
        0
    } else {
        let cleaned = strip_underscores(exp)?;
        cleaned.parse::<i64>().ok()?
    };
    let (int_part, frac_part) = split_at_dot(mantissa);
    let int_clean = strip_underscores(int_part)?;
    let frac_clean = strip_underscores(frac_part)?;
    if int_clean.is_empty() && frac_clean.is_empty() {
        return None;
    }
    let digits = format!("{}{}", int_clean, frac_clean);
    if digits.is_empty() {
        return None;
    }
    let mantissa_int = UBig::from_str_radix(&digits, 10).ok()?;
    let mantissa_int = IBig::from(mantissa_int);
    let final_exp = exp - frac_clean.len() as i64;
    Some(decimal_to_value(mantissa_int, final_exp))
}

/// Build a `Value` from a decimal `mantissa * 10^exp`, choosing between
/// [`Value::Rat`] and [`Value::Float`] based on smallness, mirroring Go's
/// `makeFloatFromLiteral`.
fn decimal_to_value(mantissa: IBig, exp: i64) -> Value {
    // Try the exact rational path first.
    if let Some(r) = build_rational_pow10(mantissa.clone(), exp) {
        if rational_is_small(&r) {
            return make_rat(r);
        }
    }
    // Fall back to BinFloat — parse with the same rounding mode as BinFloat
    // (HalfEven), then convert from base 10 to base 2.
    let dbig_str = format!("{}e{}", mantissa, exp);
    type DBigE = dashu::float::FBig<dashu::float::round::mode::HalfEven, 10>;
    let dec = match DBigE::from_str(&dbig_str) {
        Ok(d) => d,
        Err(_) => return Value::Unknown,
    };
    let bin: BinFloat = match dec.with_base::<2>() {
        dashu::base::Approximation::Exact(b) => b,
        dashu::base::Approximation::Inexact(b, _) => b,
    };
    Value::Float(bin.with_precision(PREC).value())
}

fn build_rational_pow10(mantissa: IBig, exp: i64) -> Option<RBig> {
    if exp >= 0 {
        let e = usize::try_from(exp).ok()?;
        let num = mantissa * pow10(e);
        return Some(RBig::from(num));
    }
    let e = usize::try_from(-exp).ok()?;
    let den = pow10_ubig(e);
    Some(RBig::from_parts(mantissa, den))
}

fn pow10(n: usize) -> IBig {
    IBig::from(10u32).pow(n)
}

fn pow10_ubig(n: usize) -> UBig {
    UBig::from(10u32).pow(n)
}

/// True iff `r`'s numerator and denominator are both "small" enough to keep
/// in rational form rather than promote to [`BinFloat`].
fn rational_is_small(r: &RBig) -> bool {
    small_int(r.numerator()) && small_int(&IBig::from(r.denominator().clone()))
}

/// Split `s` at the last occurrence of any character in `markers` (e.g. `e`
/// for decimal float exponent). Returns `(prefix, suffix_after_marker)`, or
/// `(s, "")` if no marker is present.
fn split_exponent<'a>(s: &'a str, markers: &[char]) -> Option<(&'a str, &'a str)> {
    if let Some(idx) = s.rfind(markers) {
        let (a, b) = s.split_at(idx);
        // Skip the marker character itself.
        let suffix = &b[b.chars().next()?.len_utf8()..];
        Some((a, suffix))
    } else {
        Some((s, ""))
    }
}

/// Split a float mantissa at its decimal/hex point. Returns
/// `(int_part, frac_part)`; either may be empty.
fn split_at_dot(s: &str) -> (&str, &str) {
    match s.find('.') {
        Some(idx) => {
            let (a, b) = s.split_at(idx);
            (a, &b[1..])
        }
        None => (s, ""),
    }
}

// ----------------------------------------------------------------------------
// Hex floats: 0x<hex_digits>[.<hex_digits>]p[+-]?<dec_digits>

fn parse_hex_float(s: &str) -> Option<Value> {
    let (mantissa, exp_str) = split_exponent(s, &['p', 'P'])?;
    if exp_str.is_empty() {
        // Hex floats require an exponent in Go.
        return None;
    }
    let exp: i64 = strip_underscores(exp_str)?.parse().ok()?;
    let (int_part, frac_part) = split_at_dot(mantissa);
    let int_clean = strip_underscores(int_part)?;
    let frac_clean = strip_underscores(frac_part)?;
    if int_clean.is_empty() && frac_clean.is_empty() {
        return None;
    }
    let hex_digits = format!("{}{}", int_clean, frac_clean);
    let mantissa_int = UBig::from_str_radix(&hex_digits, 16).ok()?;
    let mantissa_int = IBig::from(mantissa_int);
    // Each hex frac digit is 4 binary bits.
    let final_exp = exp - 4 * frac_clean.len() as i64;
    Some(binary_to_value(mantissa_int, final_exp))
}

/// Build a Value from `mantissa * 2^exp`.
fn binary_to_value(mantissa: IBig, exp: i64) -> Value {
    if exp >= 0 {
        let e = match usize::try_from(exp) {
            Ok(e) => e,
            Err(_) => return Value::Float(ibig_to_fbig(mantissa)),
        };
        let shifted = mantissa << e;
        if small_int(&shifted) {
            return make_int(shifted);
        }
        return Value::Float(ibig_to_fbig(shifted));
    }
    let e = match usize::try_from(-exp) {
        Ok(e) => e,
        Err(_) => return Value::Float(ibig_to_fbig(mantissa)),
    };
    let mut den = UBig::ZERO;
    den.set_bit(e);
    let r = RBig::from_parts(mantissa, den);
    if rational_is_small(&r) {
        return make_rat(r);
    }
    Value::Float(rbig_to_fbig(r))
}

// ----------------------------------------------------------------------------
// Char and String literals

fn parse_char_lit(lit: &str) -> Option<Value> {
    // Must be `'<content>'` with at least 2 chars.
    if lit.len() < 2 {
        return None;
    }
    let inner = lit.strip_prefix('\'')?.strip_suffix('\'')?;
    let (decoded, rest) = decode_escape(inner.as_bytes(), b'\'')?;
    if !rest.is_empty() {
        return None; // char literal must contain exactly one rune
    }
    // A rune constant is the code point, so `'\xff'` is 255 — the same number
    // the byte escape contributes to a string.
    Some(make_int64(match decoded {
        Escaped::Byte(b) => i64::from(b),
        Escaped::Rune(ch) => ch as i64,
    }))
}

fn parse_string_lit(lit: &str) -> Option<Value> {
    // Two forms: interpreted `"..."` with escapes, or raw `` `...` ``.
    // (`\r` inside a raw string is already stripped by the scanner, as in
    // go/scanner — `strconv.Unquote` would otherwise do it here.)
    if let Some(inner) = lit.strip_prefix('`').and_then(|s| s.strip_suffix('`')) {
        return Some(make_string_bytes(inner.as_bytes()));
    }
    let inner = lit.strip_prefix('"')?.strip_suffix('"')?;
    let mut out: Vec<u8> = Vec::with_capacity(inner.len());
    let mut remaining = inner.as_bytes();
    while !remaining.is_empty() {
        let (decoded, rest) = decode_escape(remaining, b'"')?;
        match decoded {
            Escaped::Byte(b) => out.push(b),
            Escaped::Rune(ch) => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
        }
        remaining = rest;
    }
    Some(make_string_bytes(out))
}

/// What one escape sequence contributes to a string literal.
///
/// Go's `strconv.UnquoteChar` returns a `multibyte` flag alongside the rune
/// for exactly this reason: `\xff` and `\377` name a **byte**, and appending
/// them as a rune would UTF-8 encode 0xFF into the two bytes 0xC3 0xBF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Escaped {
    Byte(u8),
    Rune(char),
}

/// Decode one rune from `s`, handling a single Go escape sequence if `s`
/// starts with `\`. The `quote` argument is the surrounding quote character
/// (`'` or `"`) and must be escaped inside the literal.
///
/// Bytes rather than `&str` so that `\xff` survives; the non-escape path still
/// decodes a whole rune, because the caller's input came from a `&str` and is
/// therefore well-formed UTF-8 there.
///
/// Returns `(decoded, remaining_bytes)` on success.
fn decode_escape(s: &[u8], quote: u8) -> Option<(Escaped, &[u8])> {
    let first = *s.first()?;
    if first != b'\\' {
        if first == quote {
            // An unescaped quote character is invalid inside the literal body.
            return None;
        }
        // Bounded: `decode_rune` looks at the leading sequence only, so a long
        // literal is not re-validated once per character.
        let (ch, width) = crate::utf8::decode_rune(s);
        return Some((Escaped::Rune(ch?), &s[width..]));
    }
    // Escape sequence: \x..
    let after_backslash = &s[1..];
    let esc = *after_backslash.first()?;
    let after_esc = &after_backslash[1..];
    let (decoded, rest) = match esc {
        b'a' => (Escaped::Rune('\x07'), after_esc),
        b'b' => (Escaped::Rune('\x08'), after_esc),
        b'f' => (Escaped::Rune('\x0c'), after_esc),
        b'n' => (Escaped::Rune('\n'), after_esc),
        b'r' => (Escaped::Rune('\r'), after_esc),
        b't' => (Escaped::Rune('\t'), after_esc),
        b'v' => (Escaped::Rune('\x0b'), after_esc),
        b'\\' => (Escaped::Rune('\\'), after_esc),
        b'\'' | b'"' => (Escaped::Rune(esc as char), after_esc),
        b'x' => decode_hex_byte_escape(after_esc)?,
        b'u' => decode_unicode_escape(after_esc, 4)?,
        b'U' => decode_unicode_escape(after_esc, 8)?,
        b'0'..=b'7' => decode_octal_escape(esc, after_esc)?,
        _ => return None,
    };
    Some((decoded, rest))
}

/// `\xNN` — two hex digits naming one raw byte, not a code point.
fn decode_hex_byte_escape(s: &[u8]) -> Option<(Escaped, &[u8])> {
    let digits = s.get(..2)?;
    let value = hex_value(digits)?;
    Some((Escaped::Byte(value as u8), &s[2..]))
}

/// `\uNNNN` / `\UNNNNNNNN` — a code point, UTF-8 encoded by the caller. Go
/// rejects surrogate halves and anything above U+10FFFF, and so does
/// `char::from_u32`.
fn decode_unicode_escape(s: &[u8], n: usize) -> Option<(Escaped, &[u8])> {
    let digits = s.get(..n)?;
    let code = hex_value(digits)?;
    let ch = char::from_u32(code)?;
    Some((Escaped::Rune(ch), &s[n..]))
}

/// `\NNN` — three octal digits naming one raw byte. Go rejects values above
/// 255 ("octal escape value > 255"), which is why this cannot reuse the
/// code-point path.
fn decode_octal_escape(first: u8, rest: &[u8]) -> Option<(Escaped, &[u8])> {
    let next2 = rest.get(..2)?;
    let mut value = u32::from(first - b'0');
    for &b in next2 {
        if !(b'0'..=b'7').contains(&b) {
            return None;
        }
        value = value * 8 + u32::from(b - b'0');
    }
    if value > 255 {
        return None;
    }
    Some((Escaped::Byte(value as u8), &rest[2..]))
}

/// Parse `digits` as hex. `None` unless every byte is an ASCII hex digit —
/// `u32::from_str_radix` would accept a leading `+`, and slicing a `&str` at a
/// fixed offset would panic on a multi-byte char.
fn hex_value(digits: &[u8]) -> Option<u32> {
    let mut value: u32 = 0;
    for &b in digits {
        if !b.is_ascii_hexdigit() {
            return None;
        }
        value = value * 16 + u32::from((b as char).to_digit(16)?);
    }
    Some(value)
}

// ----------------------------------------------------------------------------
// Glue for f.neg() on BinFloat — Std's `Neg` is available via the `-` op too.

trait NegInplace: Sized {
    fn neg(self) -> Self;
}
impl NegInplace for BinFloat {
    fn neg(self) -> Self {
        -self
    }
}
