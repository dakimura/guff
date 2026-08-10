//! Port of Go's `strconv.Quote` and the `unicode.IsPrint` predicate it uses.
//!
//! `net/url` renders every error through `%q`, so a URL whose message is
//! otherwise correct still differs from upstream if the quoting does. Rust's
//! `{:?}` is close but not the same: it escapes `\u{a0}` where Go writes
//! ` `, and it prints `\'` for a single quote inside a double-quoted
//! string.

use super::isprint_table::PRINT_RANGES;

const LOWERHEX: &[u8; 16] = b"0123456789abcdef";

/// Mirrors `strconv.IsPrint`: categories L, M, N, P and S, plus ASCII space.
///
/// The ranges are Go's own, generated from the Go toolchain rather than read
/// off a Unicode-category crate. Those disagree: `unicode.IsPrint` answers for
/// the Unicode version Go's tables are pinned to, and any crate on a different
/// version calls thousands of newly-assigned code points printable that Go does
/// not. Go's `strconv` ships a generated table for the same reason.
pub fn is_print(c: char) -> bool {
    let n = u32::from(c);
    PRINT_RANGES
        .binary_search_by(|&(lo, hi)| {
            if n < lo {
                std::cmp::Ordering::Greater
            } else if n > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Mirrors `strconv.Quote`.
pub fn quote(s: &str) -> String {
    quote_bytes(s.as_bytes())
}

/// [`quote`] over raw bytes. A byte that does not start a valid UTF-8 sequence
/// becomes `\xNN`, as `strconv.Quote` does for `utf8.RuneError` of width 1 —
/// `net/url` can hand back such a string after unescaping `%FF` in a host.
pub fn quote_bytes(s: &[u8]) -> String {
    let mut buf = String::with_capacity(s.len() + 2);
    buf.push('"');
    let mut i = 0;
    while i < s.len() {
        match next_rune(&s[i..]) {
            Some((c, width)) => {
                append_escaped_rune(&mut buf, c);
                i += width;
            }
            None => {
                buf.push_str("\\x");
                push_hex(&mut buf, u32::from(s[i]), 4);
                i += 1;
            }
        }
    }
    buf.push('"');
    buf
}

/// The first rune of `s`, or `None` if `s` does not start a valid sequence.
fn next_rune(s: &[u8]) -> Option<(char, usize)> {
    let head = &s[..s.len().min(4)];
    let valid = match std::str::from_utf8(head) {
        Ok(v) => v,
        Err(e) if e.valid_up_to() > 0 => std::str::from_utf8(&head[..e.valid_up_to()]).ok()?,
        Err(_) => return None,
    };
    let c = valid.chars().next()?;
    Some((c, c.len_utf8()))
}

fn append_escaped_rune(buf: &mut String, r: char) {
    if r == '"' || r == '\\' {
        buf.push('\\');
        buf.push(r);
        return;
    }
    if is_print(r) {
        buf.push(r);
        return;
    }
    match r {
        '\u{7}' => buf.push_str("\\a"),
        '\u{8}' => buf.push_str("\\b"),
        '\u{c}' => buf.push_str("\\f"),
        '\n' => buf.push_str("\\n"),
        '\r' => buf.push_str("\\r"),
        '\t' => buf.push_str("\\t"),
        '\u{b}' => buf.push_str("\\v"),
        _ => {
            let n = u32::from(r);
            if n < 0x20 || n == 0x7f {
                buf.push_str("\\x");
                push_hex(buf, n, 4);
            } else if n < 0x10000 {
                buf.push_str("\\u");
                push_hex(buf, n, 12);
            } else {
                buf.push_str("\\U");
                push_hex(buf, n, 28);
            }
        }
    }
}

/// Append the nibbles of `n` from bit `top` down to bit 0, most significant
/// first — Go's `for s := top; s >= 0; s -= 4` loop.
fn push_hex(buf: &mut String, n: u32, top: u32) {
    let mut shift = top as i32;
    while shift >= 0 {
        buf.push(LOWERHEX[((n >> shift) & 0xF) as usize] as char);
        shift -= 4;
    }
}

// ---------------------------------------------------------------------------
// Parsing. `text/template` runs every literal it lexes through these: string
// and character constants through `Unquote`/`UnquoteChar`, numbers through
// `ParseUint`/`ParseInt`/`ParseFloat`. What surfaces from a template is either
// the bare `ErrSyntax` text or a `NumError` built by `fmt`'s complex scanner,
// so both the accepted set and the wording have to be Go's.
// ---------------------------------------------------------------------------

/// The two sentinel errors of Go's `strconv`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumError {
    /// `strconv.ErrSyntax`.
    Syntax,
    /// `strconv.ErrRange`.
    Range,
}

impl NumError {
    /// The bare `err.Error()` text, which is what `text/template` prints when
    /// `Unquote` fails on a literal.
    pub fn text(self) -> &'static str {
        match self {
            NumError::Syntax => "invalid syntax",
            NumError::Range => "value out of range",
        }
    }
}

/// `(&strconv.NumError{Func: func, Num: num, Err: err}).Error()`.
pub fn num_error(func: &str, num: &str, err: NumError) -> String {
    format!("strconv.{func}: parsing {}: {}", quote(num), err.text())
}

fn lower(c: u8) -> u8 {
    c | b'\x20'
}

/// Mirrors `strconv.underscoreOK`: an underscore must sit between digits.
fn underscore_ok(s: &str) -> bool {
    let mut b = s.as_bytes();
    // saw: b'^' start, b'0' digit or base prefix, b'_' underscore, b'!' other.
    let mut saw = b'^';
    let mut i = 0;

    if !b.is_empty() && (b[0] == b'-' || b[0] == b'+') {
        b = &b[1..];
    }

    let mut hex = false;
    if b.len() >= 2
        && b[0] == b'0'
        && (lower(b[1]) == b'b' || lower(b[1]) == b'o' || lower(b[1]) == b'x')
    {
        i = 2;
        saw = b'0';
        hex = lower(b[1]) == b'x';
    }

    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() || (hex && (b'a'..=b'f').contains(&lower(c))) {
            saw = b'0';
        } else if c == b'_' {
            if saw != b'0' {
                return false;
            }
            saw = b'_';
        } else if saw == b'_' {
            return false;
        } else {
            saw = b'!';
        }
        i += 1;
    }
    saw != b'_'
}

/// Mirrors `strconv.ParseUint(s, base, bit_size)` for `base` 0 or 2..=36.
pub fn parse_uint(s: &str, base: u32, bit_size: u32) -> Result<u64, NumError> {
    if s.is_empty() {
        return Err(NumError::Syntax);
    }
    let base0 = base == 0;
    let s0 = s;
    let mut b = s.as_bytes();
    let mut base = base;
    if base0 {
        base = 10;
        if b[0] == b'0' {
            if b.len() >= 3 && lower(b[1]) == b'b' {
                base = 2;
                b = &b[2..];
            } else if b.len() >= 3 && lower(b[1]) == b'o' {
                base = 8;
                b = &b[2..];
            } else if b.len() >= 3 && lower(b[1]) == b'x' {
                base = 16;
                b = &b[2..];
            } else {
                base = 8;
                b = &b[1..];
            }
        }
    }

    let cutoff = u64::MAX / u64::from(base) + 1;
    let max_val = if bit_size >= 64 {
        u64::MAX
    } else {
        (1u64 << bit_size) - 1
    };

    let mut underscores = false;
    let mut n: u64 = 0;
    for &c in b {
        let d = if c == b'_' && base0 {
            underscores = true;
            continue;
        } else if c.is_ascii_digit() {
            c - b'0'
        } else if (b'a'..=b'z').contains(&lower(c)) {
            lower(c) - b'a' + 10
        } else {
            return Err(NumError::Syntax);
        };
        if u32::from(d) >= base {
            return Err(NumError::Syntax);
        }
        if n >= cutoff {
            return Err(NumError::Range);
        }
        n *= u64::from(base);
        let n1 = n.wrapping_add(u64::from(d));
        if n1 < n || n1 > max_val {
            return Err(NumError::Range);
        }
        n = n1;
    }

    if underscores && !underscore_ok(s0) {
        return Err(NumError::Syntax);
    }
    Ok(n)
}

/// Mirrors `strconv.ParseInt(s, base, bit_size)`.
pub fn parse_int(s: &str, base: u32, bit_size: u32) -> Result<i64, NumError> {
    if s.is_empty() {
        return Err(NumError::Syntax);
    }
    let mut rest = s;
    let mut neg = false;
    if let Some(stripped) = rest.strip_prefix('+') {
        rest = stripped;
    } else if let Some(stripped) = rest.strip_prefix('-') {
        rest = stripped;
        neg = true;
    }

    // Go keeps going on ErrRange so that the sign can still be checked, and
    // reports ErrRange either way; any other error returns immediately.
    let (un, saturated) = match parse_uint(rest, base, bit_size) {
        Ok(v) => (v, false),
        Err(NumError::Range) => (u64::MAX >> (64 - bit_size), true),
        Err(e) => return Err(e),
    };

    let cutoff = 1u64 << (bit_size - 1);
    if (!neg && un >= cutoff) || (neg && un > cutoff) || saturated {
        return Err(NumError::Range);
    }
    let n = un as i64;
    Ok(if neg { -n } else { n })
}

/// Mirrors `strconv.ParseFloat(s, 64)`.
///
/// The value is only as precise as Rust's own parser. Nothing in
/// `text/template` reads it — only whether there was an error and which one —
/// so what has to be Go's here is the accepted set and the overflow boundary.
pub fn parse_float(s: &str) -> Result<f64, NumError> {
    if let Some((v, n)) = float_special(s) {
        if n == s.len() {
            return Ok(v);
        }
    }
    let Some(read) = read_float(s) else {
        return Err(NumError::Syntax);
    };
    if read.consumed != s.len() {
        return Err(NumError::Syntax);
    }
    let value = if read.hex {
        let v = read.mantissa as f64 * exp2(read.exp);
        if read.neg {
            -v
        } else {
            v
        }
    } else {
        let cleaned: String = s.chars().filter(|&c| c != '_').collect();
        cleaned.parse::<f64>().map_err(|_| NumError::Syntax)?
    };
    if value.is_infinite() {
        // atof64 and atofHex both report only overflow; underflow returns a
        // denormal or zero with no error.
        return Err(NumError::Range);
    }
    Ok(value)
}

/// `2^exp`, saturating to infinity and zero at the ends of the range.
fn exp2(exp: i32) -> f64 {
    if exp > 1100 {
        return f64::INFINITY;
    }
    if exp < -1100 {
        return 0.0;
    }
    let (mut e, step) = if exp >= 0 {
        (exp, 2.0f64)
    } else {
        (-exp, 0.5f64)
    };
    let mut v = 1.0f64;
    while e > 0 {
        v *= step;
        e -= 1;
    }
    v
}

/// Mirrors `strconv.special`: `nan`, `inf` and `infinity`, case-insensitive.
fn float_special(s: &str) -> Option<(f64, usize)> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let (sign, nsign, rest) = match b[0] {
        b'+' => (1.0, 1, &b[1..]),
        b'-' => (-1.0, 1, &b[1..]),
        _ => (1.0, 0, b),
    };
    match rest.first().map(|c| lower(*c)) {
        Some(b'i') => {
            let mut n = common_prefix_len_ignore_case(rest, b"infinity");
            // Anything longer than "inf" is fine, but short of "infinity" only
            // "inf" is consumed.
            if 3 < n && n < 8 {
                n = 3;
            }
            if n == 3 || n == 8 {
                return Some((sign * f64::INFINITY, nsign + n));
            }
            None
        }
        // Go's sign case falls through to the inf case, not past it, so a
        // signed NaN is not special.
        Some(b'n') if nsign == 0 && common_prefix_len_ignore_case(rest, b"nan") == 3 => {
            Some((f64::NAN, 3))
        }
        _ => None,
    }
}

fn common_prefix_len_ignore_case(s: &[u8], prefix: &[u8]) -> usize {
    let n = s.len().min(prefix.len());
    let mut i = 0;
    while i < n && lower(s[i]) == prefix[i] {
        i += 1;
    }
    i
}

struct ReadFloat {
    mantissa: u64,
    exp: i32,
    neg: bool,
    hex: bool,
    consumed: usize,
}

/// Mirrors `strconv.readFloat`: the syntax half of `ParseFloat`.
fn read_float(s: &str) -> Option<ReadFloat> {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut underscores = false;
    let mut neg = false;

    if i >= b.len() {
        return None;
    }
    match b[i] {
        b'+' => i += 1,
        b'-' => {
            i += 1;
            neg = true;
        }
        _ => {}
    }

    let mut base = 10u64;
    let mut max_mant_digits = 19;
    let mut exp_char = b'e';
    let mut hex = false;
    if i + 2 < b.len() && b[i] == b'0' && lower(b[i + 1]) == b'x' {
        base = 16;
        max_mant_digits = 16;
        i += 2;
        exp_char = b'p';
        hex = true;
    }

    let mut sawdot = false;
    let mut sawdigits = false;
    let mut nd = 0i32;
    let mut nd_mant = 0i32;
    let mut dp = 0i32;
    let mut mantissa = 0u64;

    while i < b.len() {
        let c = b[i];
        if c == b'_' {
            underscores = true;
            i += 1;
            continue;
        }
        if c == b'.' {
            if sawdot {
                break;
            }
            sawdot = true;
            dp = nd;
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            sawdigits = true;
            if c == b'0' && nd == 0 {
                dp -= 1; // ignore leading zeros
                i += 1;
                continue;
            }
            nd += 1;
            if nd_mant < max_mant_digits {
                mantissa = mantissa.wrapping_mul(base);
                mantissa = mantissa.wrapping_add(u64::from(c - b'0'));
                nd_mant += 1;
            }
            i += 1;
            continue;
        }
        if base == 16 && (b'a'..=b'f').contains(&lower(c)) {
            sawdigits = true;
            nd += 1;
            if nd_mant < max_mant_digits {
                mantissa = mantissa.wrapping_mul(16);
                mantissa = mantissa.wrapping_add(u64::from(lower(c) - b'a' + 10));
                nd_mant += 1;
            }
            i += 1;
            continue;
        }
        break;
    }

    if !sawdigits {
        return None;
    }
    if !sawdot {
        dp = nd;
    }
    if base == 16 {
        dp *= 4;
        nd_mant *= 4;
    }

    if i < b.len() && lower(b[i]) == exp_char {
        i += 1;
        if i >= b.len() {
            return None;
        }
        let mut esign = 1i32;
        match b[i] {
            b'+' => i += 1,
            b'-' => {
                i += 1;
                esign = -1;
            }
            _ => {}
        }
        if i >= b.len() || !b[i].is_ascii_digit() {
            return None;
        }
        let mut e = 0i32;
        while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_') {
            if b[i] == b'_' {
                underscores = true;
                i += 1;
                continue;
            }
            if e < 10000 {
                e = e * 10 + i32::from(b[i] - b'0');
            }
            i += 1;
        }
        dp += e * esign;
    } else if base == 16 {
        return None; // a hex mantissa must carry an exponent
    }

    let exp = if mantissa != 0 { dp - nd_mant } else { 0 };

    if underscores && !underscore_ok(&s[..i]) {
        return None;
    }

    Some(ReadFloat {
        mantissa,
        exp,
        neg,
        hex,
        consumed: i,
    })
}

/// Mirrors `strconv.UnquoteChar`, returning `(value, multibyte, tail)`.
pub fn unquote_char(s: &str, quote: u8) -> Result<(char, bool, &str), NumError> {
    let b = s.as_bytes();
    if b.is_empty() {
        return Err(NumError::Syntax);
    }
    let c = b[0];
    if c == quote && (quote == b'\'' || quote == b'"') {
        return Err(NumError::Syntax);
    }
    if c >= 0x80 {
        let r = s.chars().next().ok_or(NumError::Syntax)?;
        return Ok((r, true, &s[r.len_utf8()..]));
    }
    if c != b'\\' {
        return Ok((char::from(c), false, &s[1..]));
    }

    if b.len() <= 1 {
        return Err(NumError::Syntax);
    }
    let c = b[1];
    let mut rest = &s[2..];

    let value: char;
    let mut multibyte = false;
    match c {
        b'a' => value = '\u{7}',
        b'b' => value = '\u{8}',
        b'f' => value = '\u{c}',
        b'n' => value = '\n',
        b'r' => value = '\r',
        b't' => value = '\t',
        b'v' => value = '\u{b}',
        b'x' | b'u' | b'U' => {
            let n = match c {
                b'x' => 2,
                b'u' => 4,
                _ => 8,
            };
            let rb = rest.as_bytes();
            if rb.len() < n {
                return Err(NumError::Syntax);
            }
            let mut v: u32 = 0;
            for &d in &rb[..n] {
                let x = match d {
                    b'0'..=b'9' => u32::from(d - b'0'),
                    b'a'..=b'f' => u32::from(d - b'a' + 10),
                    b'A'..=b'F' => u32::from(d - b'A' + 10),
                    _ => return Err(NumError::Syntax),
                };
                v = v << 4 | x;
            }
            rest = &rest[n..];
            if c == b'x' {
                // A single byte, possibly not UTF-8. char::from truncates to
                // Latin-1 here, which is what the caller renders; no template
                // error text carries the value itself.
                return Ok((char::from(v as u8), false, rest));
            }
            // utf8.ValidRune: surrogates and out-of-range values are rejected.
            let Some(r) = char::from_u32(v) else {
                return Err(NumError::Syntax);
            };
            value = r;
            multibyte = true;
        }
        b'0'..=b'7' => {
            let mut v = u32::from(c - b'0');
            let rb = rest.as_bytes();
            if rb.len() < 2 {
                return Err(NumError::Syntax);
            }
            for &d in &rb[..2] {
                if !(b'0'..=b'7').contains(&d) {
                    return Err(NumError::Syntax);
                }
                v = (v << 3) | u32::from(d - b'0');
            }
            rest = &rest[2..];
            if v > 255 {
                return Err(NumError::Syntax);
            }
            value = char::from(v as u8);
        }
        b'\\' => value = '\\',
        b'\'' | b'"' => {
            if c != quote {
                return Err(NumError::Syntax);
            }
            value = char::from(c);
        }
        _ => return Err(NumError::Syntax),
    }
    Ok((value, multibyte, rest))
}

/// Mirrors `strconv.Unquote`.
pub fn unquote(s: &str) -> Result<String, NumError> {
    let b = s.as_bytes();
    if b.len() < 2 {
        return Err(NumError::Syntax);
    }
    let quote = b[0];
    let Some(end) = b[1..].iter().position(|&c| c == quote) else {
        return Err(NumError::Syntax);
    };
    let end = end + 2; // one past the terminating quote

    match quote {
        b'`' => {
            if end != s.len() {
                return Err(NumError::Syntax); // trailing bytes after the quote
            }
            // Carriage returns are dropped from a raw string's value.
            Ok(s[1..end - 1].chars().filter(|&c| c != '\r').collect())
        }
        b'"' | b'\'' => {
            let mut out = String::new();
            let mut rest = &s[1..];
            while !rest.is_empty() && rest.as_bytes()[0] != quote {
                if rest.as_bytes()[0] == b'\n' {
                    return Err(NumError::Syntax);
                }
                let (r, _multibyte, next) = unquote_char(rest, quote)?;
                rest = next;
                out.push(r);
                if quote == b'\'' {
                    break; // a rune literal holds exactly one character
                }
            }
            if rest.as_bytes().first() != Some(&quote) {
                return Err(NumError::Syntax);
            }
            rest = &rest[1..];
            if !rest.is_empty() {
                return Err(NumError::Syntax);
            }
            Ok(out)
        }
        _ => Err(NumError::Syntax),
    }
}
