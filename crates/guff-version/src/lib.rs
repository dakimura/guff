//! guff-version — a Rust port of Go's `go/version` package.
//!
//! Provides operations on Go versions in Go toolchain name syntax: strings
//! like `"go1.20"`, `"go1.21.0"`, `"go1.22rc2"`, and `"go1.23.4-custom"`.
//!
//! See [`guff_gover`] for the underlying parser, which works on the same
//! versions but without the leading `"go"` prefix.
//!
//! Original Go source:
//!   Copyright 2023 The Go Authors. All rights reserved.
//!   Use of this source code is governed by a BSD-style license.

/// Convert a `"go1.21-custom"`-style version to its `"1.21"` form. Returns the
/// empty string (a known invalid version) if `v` does not start with `"go"`.
fn strip_go(v: &str) -> &str {
    // Strip `-custom` suffix.
    let head = match v.find('-') {
        Some(idx) => &v[..idx],
        None => v,
    };
    if head.len() < 2 || !head.starts_with("go") {
        return "";
    }
    &head[2..]
}

/// Returns the Go language version for version `x`. Returns the empty string
/// if `x` is not a valid version.
///
/// Examples:
/// - `lang("go1.21rc2") == "go1.21"`
/// - `lang("go1.21.2")  == "go1.21"`
/// - `lang("go1.21")    == "go1.21"`
/// - `lang("go1")       == "go1"`
/// - `lang("bad")       == ""`
/// - `lang("1.21")      == ""`
///
/// Equivalent to `go/version.Lang`.
pub fn lang(x: &str) -> String {
    let v = guff_gover::lang(strip_go(x));
    if v.is_empty() {
        return String::new();
    }
    // Match Go's behavior: when the stripped lang version is a prefix of
    // x[2:], reuse x's original spelling (e.g. preserve `"go1.21"` rather
    // than re-formatting). Otherwise reconstruct `"go" + v`.
    let after_go = &x[2..];
    if after_go.starts_with(&v) {
        x[..2 + v.len()].to_string()
    } else {
        format!("go{}", v)
    }
}

/// Returns -1, 0, or +1 depending on whether `x < y`, `x == y`, or `x > y`,
/// interpreted as Go versions.
///
/// The versions must begin with a `"go"` prefix (e.g. `"go1.21"`, not
/// `"1.21"`). Invalid versions (including the empty string) compare less than
/// valid versions and equal to each other. The language version `"go1.21"`
/// compares less than the release candidate and eventual releases
/// `"go1.21rc1"` and `"go1.21.0"`.
///
/// Equivalent to `go/version.Compare`.
pub fn compare(x: &str, y: &str) -> i32 {
    guff_gover::compare(strip_go(x), strip_go(y))
}

/// Reports whether the version `x` is valid.
///
/// Equivalent to `go/version.IsValid`.
pub fn is_valid(x: &str) -> bool {
    guff_gover::is_valid(strip_go(x))
}
