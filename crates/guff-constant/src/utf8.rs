//! The part of Go's `unicode/utf8` that decides how ill-formed bytes become
//! U+FFFD.
//!
//! Rust's `String::from_utf8_lossy` is **not** a substitute. It follows the
//! Unicode "maximal subpart" recommendation and emits one U+FFFD per truncated
//! sequence; `utf8.DecodeRune` returns `(RuneError, 1)` and so Go emits one per
//! *byte*. On `"\xe0\xa0"` Rust says one replacement character and Go says two.
//!
//! That difference is user-visible in both directions: `[]rune(s)` is how
//! SA1024 counts a cutset, and `encoding/json` uses the same rule when
//! golangci-lint writes a message containing raw bytes — so the count decides
//! whether guff's output matches the golden byte for byte.

/// `utf8.DecodeRune`: the leading rune of `s` and its width in bytes.
///
/// `None` marks a byte that starts no well-formed rune, which Go decodes as
/// `(RuneError, 1)`.
///
/// # Panics
/// Panics if `s` is empty.
pub fn decode_rune(s: &[u8]) -> (Option<char>, usize) {
    let width = seq_len(s[0]);
    if width == 0 || s.len() < width {
        return (None, 1);
    }
    // `from_utf8` rejects exactly what Go's accept ranges reject: a bad
    // continuation byte, an overlong encoding, a surrogate half, and anything
    // above U+10FFFF.
    match std::str::from_utf8(&s[..width]) {
        Ok(text) => (text.chars().next(), width),
        Err(_) => (None, 1),
    }
}

/// Byte count of the UTF-8 sequence `b` starts, or 0 if it starts none.
fn seq_len(b: u8) -> usize {
    match b {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 0,
    }
}

/// The Go `string(...[]rune(s)...)` round trip: every byte that starts no
/// well-formed rune becomes U+FFFD.
///
/// This is what a Go program sees when it ranges over a string, converts one
/// to `[]rune`, or marshals one to JSON.
pub fn decode_lossy(s: &[u8]) -> String {
    // Fast path: the overwhelmingly common case is already valid.
    if let Ok(text) = std::str::from_utf8(s) {
        return text.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        let (ch, width) = decode_rune(rest);
        out.push(ch.unwrap_or(char::REPLACEMENT_CHARACTER));
        rest = &rest[width..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_utf8_is_unchanged() {
        assert_eq!(decode_lossy(b"hello"), "hello");
        assert_eq!(decode_lossy("snow\u{2603}man".as_bytes()), "snow\u{2603}man");
    }

    #[test]
    fn one_replacement_per_ill_formed_byte() {
        // Go: `for range` over these yields one U+FFFD per byte. Rust's
        // from_utf8_lossy yields one for the whole truncated sequence, which is
        // the bug this module exists to avoid.
        assert_eq!(decode_lossy(b"\xff").chars().count(), 1);
        assert_eq!(decode_lossy(b"\xe0\xa0").chars().count(), 2);
        assert_eq!(decode_lossy(b"\xed\xa0\x80").chars().count(), 3);
        assert_eq!(decode_lossy(b"\xf0\x9f").chars().count(), 2);
        assert_eq!(decode_lossy(b"a\xffb"), "a\u{fffd}b");
    }

    #[test]
    fn decode_rune_widths_match_go() {
        assert_eq!(decode_rune(b"a"), (Some('a'), 1));
        assert_eq!(decode_rune("\u{2603}".as_bytes()), (Some('\u{2603}'), 3));
        // Overlong, surrogate, and out-of-range leads are all (RuneError, 1).
        assert_eq!(decode_rune(b"\xc0\x80"), (None, 1));
        assert_eq!(decode_rune(b"\xed\xa0\x80"), (None, 1));
        assert_eq!(decode_rune(b"\xf5\x80\x80\x80"), (None, 1));
        // A well-formed lead with too few bytes left is also (RuneError, 1).
        assert_eq!(decode_rune(b"\xe2\x98"), (None, 1));
    }
}
