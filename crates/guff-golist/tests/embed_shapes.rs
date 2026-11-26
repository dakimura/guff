//! The `//go:embed` shapes, end to end: directory scan → patterns → resolution.
//!
//! Every expectation here was read off `go list -e -test
//! -json=EmbedPatterns,EmbedFiles,Error` on the same tree (with a `go.mod`
//! added), and golangci-lint 2.12.2 reports the same 13 findings through its
//! `typecheck` pseudo linter. Needs no Go toolchain: the native lister is the
//! thing under test.
//!
//! The one shape both tools disagree on is `noimport/`, and it is here to
//! record that: a `//go:embed` in a package that never imports `embed` is a
//! *compiler* error (`go:embed requires import "embed"`), which golangci-lint
//! gets from the type checker and guff does not emit. `go list` is silent
//! about it, so the native lister must be silent too — which is what the
//! `noimport` row asserts.

use std::path::{Path, PathBuf};

use guff_build::Context;
use guff_golist::resolve_embed;

fn testdata() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/embed")
}

/// `(pattern, file, line, column)` for each of the three pattern sets.
type Pats = Vec<(String, String, usize, usize)>;

struct Scanned {
    prod: Pats,
    test: Pats,
    xtest: Pats,
}

fn scan(case: &str) -> Scanned {
    let pkg = Context::default()
        .import_dir(testdata().join(case))
        .unwrap_or_else(|e| panic!("import_dir({case}): {e}"));
    let conv = |v: &[guff_build::EmbedPattern]| -> Pats {
        v.iter()
            .map(|p| (p.pattern.clone(), p.file.clone(), p.line, p.column))
            .collect()
    };
    Scanned {
        prod: conv(&pkg.embed_patterns),
        test: conv(&pkg.test_embed_patterns),
        xtest: conv(&pkg.xtest_embed_patterns),
    }
}

fn pat(p: &str, f: &str, line: usize, col: usize) -> (String, String, usize, usize) {
    (p.to_string(), f.to_string(), line, col)
}

/// Resolve one case's production patterns, as `Ok(files)` / `Err(message)`.
fn resolve(case: &str, pats: &Pats) -> Result<Vec<String>, String> {
    let names: Vec<String> = pats.iter().map(|p| p.0.clone()).collect();
    resolve_embed(&testdata().join(case), &names).map_err(|e| e.text())
}

fn dir_of(case: &str) -> PathBuf {
    testdata().join(case)
}

#[track_caller]
fn assert_resolves(case: &str, want: Result<Vec<&str>, &str>) {
    let s = scan(case);
    let got = resolve(case, &s.prod);
    let got_ref: Result<Vec<&str>, &str> = match &got {
        Ok(v) => Ok(v.iter().map(String::as_str).collect()),
        Err(e) => Err(e.as_str()),
    };
    assert_eq!(got_ref, want, "case {case}");
    let _: &Path = &dir_of(case);
}

/// Every case directory is scanned, so a shape cannot be added to the tree and
/// then quietly go unasserted.
#[test]
fn every_case_directory_is_covered() {
    let mut found: Vec<String> = std::fs::read_dir(testdata())
        .expect("testdata")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    assert_eq!(
        found,
        vec![
            "allmissing",
            "allprefix",
            "badsyntax",
            "glob",
            "missingdir",
            "missingfile",
            "nofiles",
            "noimport",
            "ok",
            "quoted",
            "secondfails",
            "testonly",
            "twofiles",
            "variants",
        ]
    );
}

#[test]
fn patterns_carry_the_position_go_list_blames() {
    // `//go:embed ` is 11 bytes: the pattern starts at column 12.
    assert_eq!(
        scan("missingdir").prod,
        vec![pat("app/dist", "a.go", 5, 12)]
    );
    // Two patterns on one line each get their own column.
    assert_eq!(
        scan("secondfails").prod,
        vec![
            pat("have.txt", "a.go", 5, 12),
            pat("nope.txt", "a.go", 5, 21),
        ]
    );
    // A quoted argument is unquoted, but the column is the opening quote.
    assert_eq!(
        scan("quoted").prod,
        vec![pat("no such.txt", "a.go", 5, 12)]
    );
    // `all:` stays part of the pattern.
    assert_eq!(
        scan("allmissing").prod,
        vec![pat("all:hidden", "a.go", 5, 12)]
    );
    // A repeated pattern keeps the first file in scan order (a.go, not b.go).
    assert_eq!(scan("twofiles").prod, vec![pat("shared", "a.go", 5, 12)]);
    // `import _ "embed"` is still an import of embed.
    assert_eq!(
        scan("missingfile").prod,
        vec![pat("missing.txt", "a.go", 5, 12)]
    );
}

#[test]
fn a_directive_without_the_embed_import_is_not_a_pattern() {
    // `go list` reports nothing here; only the compiler does. If this ever
    // starts producing a pattern, guff invents a `typecheck` finding — and one
    // of those deletes every other finding in the run.
    let s = scan("noimport");
    assert_eq!(s.prod, Pats::new());
    assert_eq!(s.test, Pats::new());
    assert_eq!(s.xtest, Pats::new());
}

#[test]
fn test_and_xtest_patterns_are_kept_apart() {
    // The production package has no patterns of its own here.
    let s = scan("testonly");
    assert_eq!(s.prod, Pats::new());
    assert_eq!(s.test, vec![pat("testmissing", "a_test.go", 8, 12)]);
    assert_eq!(s.xtest, Pats::new());

    let v = scan("variants");
    assert_eq!(v.prod, vec![pat("prodmissing", "a.go", 5, 12)]);
    assert_eq!(v.test, vec![pat("testmissing", "a_test.go", 8, 12)]);
    assert_eq!(v.xtest, vec![pat("xtestmissing", "x_test.go", 8, 12)]);
}

#[test]
fn resolution_matches_go_list_on_every_shape() {
    assert_resolves("missingdir", Err("pattern app/dist: no matching files found"));
    assert_resolves(
        "missingfile",
        Err("pattern missing.txt: no matching files found"),
    );
    assert_resolves(
        "secondfails",
        Err("pattern nope.txt: no matching files found"),
    );
    assert_resolves(
        "quoted",
        Err("pattern no such.txt: no matching files found"),
    );
    assert_resolves(
        "allmissing",
        Err("pattern all:hidden: no matching files found"),
    );
    assert_resolves("glob", Err("pattern *.tmpl: no matching files found"));
    assert_resolves("twofiles", Err("pattern shared: no matching files found"));
    assert_resolves("badsyntax", Err("pattern ../ok: invalid pattern syntax"));
    assert_resolves(
        "nofiles",
        Err("pattern only: cannot embed directory only: contains no embeddable files"),
    );
    assert_resolves("variants", Err("pattern prodmissing: no matching files found"));
    // The two that succeed: a directory drops dot-names, `all:` keeps them.
    assert_resolves("ok", Ok(vec!["data/sub/y.txt", "data/x.txt"]));
    assert_resolves("allprefix", Ok(vec!["data/.hidden", "data/x.txt"]));
    // Nothing to resolve is not an error.
    assert_resolves("noimport", Ok(vec![]));
    assert_resolves("testonly", Ok(vec![]));
}

#[test]
fn the_test_variants_resolve_their_own_patterns() {
    let s = scan("testonly");
    let names: Vec<String> = s.test.iter().map(|p| p.0.clone()).collect();
    assert_eq!(
        resolve_embed(&dir_of("testonly"), &names).map_err(|e| e.text()),
        Err("pattern testmissing: no matching files found".to_string())
    );

    let v = scan("variants");
    let xnames: Vec<String> = v.xtest.iter().map(|p| p.0.clone()).collect();
    assert_eq!(
        resolve_embed(&dir_of("variants"), &xnames).map_err(|e| e.text()),
        Err("pattern xtestmissing: no matching files found".to_string())
    );
}
