//! guff-gover — a Rust port of Go's `internal/gover` package.
//!
//! Implements support for Go toolchain versions like `1.21.0` and `1.21rc1`.
//! (For historical reasons, Go does not use semver for its toolchains.)
//!
//! The [`crate::version`][crate]-like wrapper crate `guff-version` should
//! be preferred when possible. Note that this crate works on `"1.21"` while
//! `guff-version` works on `"go1.21"`.
//!
//! Original Go source:
//!   Copyright 2023 The Go Authors. All rights reserved.
//!   Use of this source code is governed by a BSD-style license.

use std::cmp::Ordering;

/// A parsed Go version: `major[.minor[.patch]][kind[pre]]`.
///
/// The numbers are kept as their original decimal strings to avoid integer
/// overflows (and because there is very little actual math). This mirrors Go's
/// rationale for `gover.Version`, which existed to support test inputs like
/// `go1.99999999999`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Version {
    /// Decimal string.
    pub major: String,
    /// Decimal string or empty.
    pub minor: String,
    /// Decimal string or empty.
    pub patch: String,
    /// `""`, `"alpha"`, `"beta"`, or `"rc"`.
    pub kind: String,
    /// Decimal string or empty.
    pub pre: String,
}

/// Returns -1, 0, or +1 depending on whether `x < y`, `x == y`, or `x > y`,
/// interpreted as toolchain versions.
///
/// `x` and `y` must **not** begin with a `go` prefix — pass `"1.21"`, not
/// `"go1.21"`. Malformed versions compare less than well-formed versions and
/// equal to each other. The language version `1.21` compares less than the
/// release candidate and eventual releases `1.21rc1` and `1.21.0`.
///
/// Equivalent to `gover.Compare`.
pub fn compare(x: &str, y: &str) -> i32 {
    let vx = parse(x);
    let vy = parse(y);

    let c = cmp_int(&vx.major, &vy.major);
    if c != 0 {
        return c;
    }
    let c = cmp_int(&vx.minor, &vy.minor);
    if c != 0 {
        return c;
    }
    let c = cmp_int(&vx.patch, &vy.patch);
    if c != 0 {
        return c;
    }
    // "" < "alpha" < "beta" < "rc" — these strings sort lexicographically the
    // right way already.
    match vx.kind.cmp(&vy.kind) {
        Ordering::Less => return -1,
        Ordering::Greater => return 1,
        Ordering::Equal => {}
    }
    cmp_int(&vx.pre, &vy.pre)
}

/// Returns the maximum of `x` and `y` interpreted as toolchain versions,
/// compared using [`compare`]. If they compare equal, returns `x`.
///
/// Equivalent to `gover.Max`.
pub fn max<'a>(x: &'a str, y: &'a str) -> &'a str {
    if compare(x, y) < 0 {
        y
    } else {
        x
    }
}

/// Reports whether `x` denotes the overall Go language version and not a
/// specific release. Starting with the Go 1.21 release, `"1.x"` denotes the
/// overall language version; the first release is `"1.x.0"`.
///
/// The distinction matters because the relative ordering is
/// `1.21 < 1.21rc1 < 1.21.0`, so Go 1.21rc1 and Go 1.21.0 both handle
/// `go.mod` files that say `go 1.21`, but Go 1.21rc1 does not handle files
/// that say `go 1.21.0`.
///
/// Equivalent to `gover.IsLang`.
pub fn is_lang(x: &str) -> bool {
    let v = parse(x);
    v != Version::default() && v.patch.is_empty() && v.kind.is_empty() && v.pre.is_empty()
}

/// Returns the Go language version. For example, `lang("1.2.3") == "1.2"`.
///
/// Equivalent to `gover.Lang`.
pub fn lang(x: &str) -> String {
    let v = parse(x);
    if v.minor.is_empty() || (v.major == "1" && v.minor == "0") {
        return v.major;
    }
    format!("{}.{}", v.major, v.minor)
}

/// Reports whether `x` is a valid version.
///
/// Equivalent to `gover.IsValid`.
pub fn is_valid(x: &str) -> bool {
    parse(x) != Version::default()
}

/// Parses the Go version string `x` into a [`Version`]. Returns the zero
/// version if `x` is malformed.
///
/// Equivalent to `gover.Parse`.
pub fn parse(x: &str) -> Version {
    let mut v = Version::default();

    // Major.
    let (major, rest) = match cut_int(x) {
        Some(p) => p,
        None => return Version::default(),
    };
    v.major = major.to_string();
    if rest.is_empty() {
        // Interpret "1" as "1.0.0".
        v.minor = "0".to_string();
        v.patch = "0".to_string();
        return v;
    }

    // Dot before minor.
    if !rest.starts_with('.') {
        return Version::default();
    }
    let rest = &rest[1..];

    // Minor.
    let (minor, rest) = match cut_int(rest) {
        Some(p) => p,
        None => return Version::default(),
    };
    v.minor = minor.to_string();
    if rest.is_empty() {
        // Patch missing is same as "0" for older versions. Starting in Go
        // 1.21, missing patch differs from explicit `.0`.
        if cmp_int(&v.minor, "21") < 0 {
            v.patch = "0".to_string();
        }
        return v;
    }

    // Patch if present.
    if rest.starts_with('.') {
        let (patch, rest2) = match cut_int(&rest[1..]) {
            Some(p) => p,
            None => return Version::default(),
        };
        if !rest2.is_empty() {
            // Disallow prereleases on patch releases (see Go source for the
            // rationale around `1.21 < 1.21rc1` ordering inversion).
            return Version::default();
        }
        v.patch = patch.to_string();
        return v;
    }

    // Prerelease kind: leading alpha run.
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() && !(b'0'..=b'9').contains(&bytes[i]) {
        if !(b'a'..=b'z').contains(&bytes[i]) {
            return Version::default();
        }
        i += 1;
    }
    if i == 0 {
        return Version::default();
    }
    v.kind = rest[..i].to_string();
    let rest = &rest[i..];
    if rest.is_empty() {
        return v;
    }
    let (pre, rest2) = match cut_int(rest) {
        Some(p) => p,
        None => return Version::default(),
    };
    if !rest2.is_empty() {
        return Version::default();
    }
    v.pre = pre.to_string();

    v
}

/// Scan the leading decimal number at the start of `x` and return the digit
/// substring plus the remainder of the string.
///
/// Returns `None` if `x` does not start with a digit, or if it has an
/// unnecessary leading zero (e.g. `"01"`).
fn cut_int(x: &str) -> Option<(&str, &str)> {
    let bytes = x.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (b'0'..=b'9').contains(&bytes[i]) {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    // No unnecessary leading zero: "01" is invalid, "0" alone is fine.
    if bytes[0] == b'0' && i != 1 {
        return None;
    }
    Some((&x[..i], &x[i..]))
}

/// Returns `cmp::Ordering`-style -1/0/+1, interpreting `x` and `y` as decimal
/// numbers. Empty string sorts as the smallest value, matching Go's behavior
/// where missing components compare smallest.
///
/// Equivalent to `gover.CmpInt`.
pub fn cmp_int(x: &str, y: &str) -> i32 {
    if x == y {
        return 0;
    }
    if x.len() < y.len() {
        return -1;
    }
    if x.len() > y.len() {
        return 1;
    }
    // Same length, lexicographic comparison gives numerical order.
    if x < y {
        -1
    } else {
        1
    }
}

/// Returns the decimal string decremented by 1, or the empty string if
/// `decimal` is all zeroes.
///
/// Equivalent to `gover.DecInt`.
pub fn dec_int(decimal: &str) -> String {
    let mut digits: Vec<u8> = decimal.bytes().collect();
    // Scan right-to-left, turning trailing 0s into 9s until we find a digit
    // we can decrement.
    let mut i = digits.len() as isize - 1;
    while i >= 0 && digits[i as usize] == b'0' {
        digits[i as usize] = b'9';
        i -= 1;
    }
    if i < 0 {
        return String::new();
    }
    let idx = i as usize;
    if idx == 0 && digits[idx] == b'1' && digits.len() > 1 {
        // Borrow turned the leading "1" into "0…9…" — drop the leading zero
        // so "10" → "9", "100" → "99", etc.
        digits.remove(0);
    } else {
        digits[idx] -= 1;
    }
    String::from_utf8(digits).expect("ASCII digits stay ASCII")
}
