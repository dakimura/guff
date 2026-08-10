//! Go's `unicode.IsLetter` and `unicode.IsDigit`, from Go's own tables.
//!
//! `text/template`'s lexer ends an identifier, field or variable at the first
//! rune that is neither of these, so they decide where a name stops and a
//! `bad character` error begins. Go answers for the Unicode version its tables
//! are pinned to, and a Rust category crate on any other version disagrees on
//! every code point assigned in between — the same trap [`super::strconv::is_print`]
//! documents. The ranges are therefore generated from the Go toolchain by
//! `compat/oracles/gotemplate` and checked against it over the whole rune space
//! by `tests/gostd_template.rs`.

use super::unicode_table::{DIGIT_RANGES, LETTER_RANGES};

fn in_ranges(ranges: &[(u32, u32)], c: char) -> bool {
    let n = u32::from(c);
    ranges
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

/// Mirrors `unicode.IsLetter`: the L category.
pub fn is_letter(c: char) -> bool {
    in_ranges(LETTER_RANGES, c)
}

/// Mirrors `unicode.IsDigit`: the Nd category.
pub fn is_digit(c: char) -> bool {
    in_ranges(DIGIT_RANGES, c)
}
