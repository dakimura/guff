//! `is_generated` over the two files the `generated-bom` golden case lints.
//!
//! The unit tests in `generated.rs` build their sources in memory, which cannot
//! catch a fixture whose bytes stop being what the test claims — a BOM is three
//! invisible bytes, and an editor that strips it would silently turn the golden
//! case into a second copy of the no-BOM case. These read the files.

use std::path::PathBuf;

use guff_fmt::{is_generated, GeneratedMode};

fn testdata(case: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/generated")
        .join(case)
        .join("a.go")
}

fn read(case: &str) -> Vec<u8> {
    std::fs::read(testdata(case)).unwrap_or_else(|e| panic!("read {case}: {e}"))
}

#[test]
fn the_bom_fixture_really_starts_with_a_bom() {
    let src = read("bomline");
    assert_eq!(
        &src[..3],
        b"\xef\xbb\xbf",
        "bomline/a.go must start with a UTF-8 BOM; without it this fixture \
         tests nothing the no-BOM case does not"
    );
    assert!(!read("blockmarker").starts_with(b"\xef\xbb\xbf"));
}

#[test]
fn a_bom_before_the_marker_is_still_generated() {
    let src = read("bomline");
    // Both modes: the BOM must not hide a `// Code generated ... DO NOT EDIT.`
    // line. `golangci-lint run` uses strict, which is what this case pins.
    assert!(is_generated(&src, GeneratedMode::Strict));
    assert!(is_generated(&src, GeneratedMode::Lax));
    assert!(!is_generated(&src, GeneratedMode::Disable));
}

#[test]
fn a_block_comment_marker_is_lax_only() {
    let src = read("blockmarker");
    // `ast.IsGenerated` wants a `//` line comment, so strict says no — which is
    // why `golangci-lint run` reports this file and guff must too.
    assert!(!is_generated(&src, GeneratedMode::Strict));
    assert!(is_generated(&src, GeneratedMode::Lax));
}
