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

