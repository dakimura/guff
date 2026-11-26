//! Integration tests for `make_from_literal`.

use guff::token::Token;
use guff_constant::{
    compare, int64_val, make_from_literal, make_int64, string_val, Kind, Value,
};

fn parse(lit: &str, tok: Token) -> Value {
    make_from_literal(lit, tok, 0)
}

// ---- INT ----

#[test]
fn int_decimal() {
    assert_eq!(int64_val(&parse("0", Token::INT)), (0, true));
    assert_eq!(int64_val(&parse("42", Token::INT)), (42, true));
    assert_eq!(int64_val(&parse("-7", Token::INT)), (-7, true));
}

#[test]
fn int_hex() {
    assert_eq!(int64_val(&parse("0xff", Token::INT)), (0xff, true));
    assert_eq!(int64_val(&parse("0X10", Token::INT)), (16, true));
}

#[test]
fn int_octal_modern() {
    assert_eq!(int64_val(&parse("0o17", Token::INT)), (15, true));
}

#[test]
fn int_octal_legacy() {
    assert_eq!(int64_val(&parse("017", Token::INT)), (15, true));
}

#[test]
fn int_binary() {
    assert_eq!(int64_val(&parse("0b1010", Token::INT)), (10, true));
}

#[test]
fn int_underscores() {
    assert_eq!(int64_val(&parse("1_000_000", Token::INT)), (1_000_000, true));
    assert_eq!(int64_val(&parse("0xff_ff", Token::INT)), (0xffff, true));
}

#[test]
fn int_invalid_underscores_yields_unknown() {
    // Leading underscore is invalid in Go.
    assert_eq!(parse("_123", Token::INT).kind(), Kind::Unknown);
    // Trailing underscore is invalid.
    assert_eq!(parse("123_", Token::INT).kind(), Kind::Unknown);
    // Double underscore is invalid.
    assert_eq!(parse("1__0", Token::INT).kind(), Kind::Unknown);
}

#[test]
fn int_above_i64_uses_ibig_variant() {
    // 2^70 > i64::MAX
    let v = parse("1180591620717411303424", Token::INT);
    assert_eq!(v.kind(), Kind::Int);
    let (_, exact) = int64_val(&v);
    assert!(!exact, "expected IBig variant");
}

// ---- FLOAT ----

#[test]
fn float_decimal_exact_rational() {
    // 1.5 → 3/2 — exact Rat
    let v = parse("1.5", Token::FLOAT);
    assert_eq!(v.kind(), Kind::Float);
    // Numerator 3, denominator 2.
    use guff_constant::{denom, num};
    assert_eq!(int64_val(&num(v.clone())), (3, true));
    assert_eq!(int64_val(&denom(v)), (2, true));
}

#[test]
fn float_with_exponent() {
    // 1.5e2 == 150 — integer, ToInt should accept.
    let v = parse("1.5e2", Token::FLOAT);
    assert!(compare(v.clone(), Token::EQL, make_int64(150)));
    let v = parse("1e-1", Token::FLOAT);
    use guff_constant::{denom, num};
    assert_eq!(int64_val(&num(v.clone())), (1, true));
    assert_eq!(int64_val(&denom(v)), (10, true));
}

#[test]
fn float_zero_forms() {
    let v = parse("0.0", Token::FLOAT);
    assert!(compare(v, Token::EQL, parse("0", Token::INT)));
    let v = parse("0.", Token::FLOAT);
    assert_eq!(v.kind(), Kind::Float);
    let v = parse(".0", Token::FLOAT);
    assert_eq!(v.kind(), Kind::Float);
}

#[test]
fn float_underscores() {
    let v = parse("1_000.5", Token::FLOAT);
    // 1000.5 == 2001/2
    assert!(compare(v, Token::EQL, parse("2001", Token::INT).clone()).not());
}

trait BoolExt {
    fn not(self) -> bool;
}
impl BoolExt for bool {
    fn not(self) -> bool {
        !self
    }
}

#[test]
fn float_hex() {
    // 0x1.8p1 == 1.5 * 2 == 3
    let v = parse("0x1.8p1", Token::FLOAT);
    assert!(compare(v, Token::EQL, make_int64(3)));
    // 0x1p4 == 16
    let v = parse("0x1p4", Token::FLOAT);
    assert!(compare(v, Token::EQL, make_int64(16)));
    // 0x1.fp4 == 31 (0x1f * 2^0)
    let v = parse("0x1.fp4", Token::FLOAT);
    assert!(compare(v, Token::EQL, make_int64(31)));
}

#[test]
fn float_negative() {
    let v = parse("-1.5", Token::FLOAT);
    assert!(compare(v, Token::EQL, parse("-3", Token::INT)).not()); // -1.5 != -3
}

#[test]
fn float_invalid_yields_unknown() {
    assert_eq!(parse("abc", Token::FLOAT).kind(), Kind::Unknown);
    assert_eq!(parse("0xfp", Token::FLOAT).kind(), Kind::Unknown); // hex needs exponent digits
}

// ---- IMAG ----

#[test]
fn imag_zero() {
    // "0i" → re=Int64(0), im=Rat(0/1). Imaginary part has Float kind because
    // it's parsed through the float-literal path (Go behaves the same way).
    let v = parse("0i", Token::IMAG);
    assert_eq!(v.kind(), Kind::Complex);
    if let Value::Complex { re, im } = v {
        assert!(compare(*re, Token::EQL, make_int64(0)));
        assert!(compare(*im, Token::EQL, make_int64(0)));
    }
}

#[test]
fn imag_nonzero() {
    let v = parse("2i", Token::IMAG);
    if let Value::Complex { re, im } = v {
        assert!(compare(*re, Token::EQL, make_int64(0)));
        assert!(compare(*im, Token::EQL, make_int64(2)));
    } else {
        panic!("expected Complex");
    }
}

#[test]
fn imag_float() {
    // 0.5i = 0 + 0.5i
    let v = parse("0.5i", Token::IMAG);
    assert_eq!(v.kind(), Kind::Complex);
}

#[test]
fn imag_missing_i_is_unknown() {
    assert_eq!(parse("2", Token::IMAG).kind(), Kind::Unknown);
}

// ---- CHAR ----

#[test]
fn char_simple_ascii() {
    let v = parse("'a'", Token::CHAR);
    assert_eq!(int64_val(&v), ('a' as i64, true));
}

#[test]
fn char_escape() {
    assert_eq!(int64_val(&parse("'\\n'", Token::CHAR)), ('\n' as i64, true));
    assert_eq!(int64_val(&parse("'\\t'", Token::CHAR)), ('\t' as i64, true));
    assert_eq!(int64_val(&parse("'\\\\'", Token::CHAR)), ('\\' as i64, true));
    assert_eq!(int64_val(&parse("'\\''", Token::CHAR)), ('\'' as i64, true));
}

#[test]
fn char_hex_escape() {
    assert_eq!(int64_val(&parse("'\\x41'", Token::CHAR)), ('A' as i64, true));
}

#[test]
fn char_unicode_escape() {
    // ☃ == ☃
    assert_eq!(int64_val(&parse("'\\u2603'", Token::CHAR)), (0x2603, true));
}

#[test]
fn char_invalid_returns_unknown() {
    assert_eq!(parse("''", Token::CHAR).kind(), Kind::Unknown);
    assert_eq!(parse("'ab'", Token::CHAR).kind(), Kind::Unknown);
}

// ---- STRING ----

#[test]
fn string_simple() {
    let v = parse("\"hello\"", Token::STRING);
    assert_eq!(string_val(&v), "hello".as_bytes());
}

#[test]
fn string_with_escapes() {
    let v = parse("\"a\\nb\"", Token::STRING);
    assert_eq!(string_val(&v), "a\nb".as_bytes());
}

#[test]
fn string_raw() {
    // Backtick-delimited strings preserve content verbatim, including
    // backslashes.
    let v = parse("`a\\nb`", Token::STRING);
    assert_eq!(string_val(&v), "a\\nb".as_bytes());
}

#[test]
fn string_empty() {
    let v = parse("\"\"", Token::STRING);
    assert_eq!(string_val(&v), "".as_bytes());
}

#[test]
fn string_unicode_escape() {
    let v = parse("\"\\u2603\"", Token::STRING);
    assert_eq!(string_val(&v), "\u{2603}".as_bytes());
}

/// A Go string is a byte string. `\xff` and `\377` each name one byte, and
/// neither is the code point U+00FF — which is two bytes, and a *different*
/// constant. Conflating them let `regexp.MustCompile("\xff")` compile and made
/// `switch` see a duplicate case where Go sees two.
#[test]
fn string_byte_escapes_are_bytes_not_code_points() {
    assert_eq!(string_val(&parse(r#""\xff""#, Token::STRING)), b"\xff");
    assert_eq!(string_val(&parse(r#""\377""#, Token::STRING)), b"\xff");
    assert_eq!(string_val(&parse(r#""\u00ff""#, Token::STRING)), b"\xc3\xbf");
    assert_ne!(
        string_val(&parse(r#""\xff""#, Token::STRING)),
        string_val(&parse(r#""\u00ff""#, Token::STRING)),
    );
    // len() is a byte count in Go.
    assert_eq!(string_val(&parse(r#""\xff""#, Token::STRING)).len(), 1);
    assert_eq!(string_val(&parse(r#""\u00ff""#, Token::STRING)).len(), 2);
}

/// A rune constant is the code point, so the byte escapes agree there.
#[test]
fn char_byte_escape_is_the_code_point() {
    assert_eq!(int64_val(&parse(r"'\xff'", Token::CHAR)), (0xff, true));
    assert_eq!(int64_val(&parse(r"'\377'", Token::CHAR)), (0xff, true));
    // Go rejects an octal escape above 255, and a surrogate half.
    assert_eq!(parse(r"'\400'", Token::CHAR).kind(), Kind::Unknown);
    assert_eq!(parse(r"'\ud800'", Token::CHAR).kind(), Kind::Unknown);
}

/// The display forms quote bytes the way `strconv.Quote` does.
#[test]
fn quoting_writes_ill_formed_bytes_as_hex() {
    let v = parse(r#""a\xffb""#, Token::STRING);
    assert_eq!(v.to_string(), r#""a\xffb""#);
    assert_eq!(v.exact_string(), r#""a\xffb""#);
}

/// Concatenation and comparison stay bytewise.
#[test]
fn byte_strings_concatenate_and_compare_bytewise() {
    use guff::token::Token as Tok;
    let a = parse(r#""\xff""#, Token::STRING);
    let b = parse(r#""\xfe""#, Token::STRING);
    assert_eq!(
        string_val(&guff_constant::binary_op(a.clone(), Tok::ADD, b.clone())),
        b"\xff\xfe"
    );
    assert!(guff_constant::compare(b.clone(), Tok::LSS, a.clone()));
}
