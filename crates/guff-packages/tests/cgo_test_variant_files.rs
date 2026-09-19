//! A cgo package's **test variant** must get its own `CompiledGoFiles`.
//!
//! `go list -compiled` is asked a second time, only for the cgo/SWIG packages,
//! because their compiled file set is not derivable from `GoFiles` (it holds
//! generated files under GOCACHE). That query used to omit `-test`, so it
//! answered for `p` and not for `p [p.test]` — and since the answer is attached
//! by id with a fall back to the import path, the variant was handed the
//! production list. Its own `_test.go` files were simply not in it.
//!
//! Nothing in a findings diff says "this file was never read". What it looks
//! like is the other tool having false positives: on elastic/beats,
//! `auditbeat/module/file_integrity` is a cgo package whose 13 in-package test
//! files produced 311 golangci-lint findings and zero guff ones (measured
//! 2026-09-20 — it was the single largest divergence in that target, 61% of
//! all of them, and the only package in beats with that shape).
//!
//! External test packages (`package p_test`) were never affected: they are a
//! separate package that does not use cgo itself.

use std::path::PathBuf;

use guff_packages::{go_available, load, Config, LoadMode};

fn testdata_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/cgotest")
}

/// `import "C"` needs a C toolchain; without one every file in the package is
/// excluded by build constraints and there is no cgo package to test.
fn cgo_available(dir: &PathBuf) -> bool {
    std::process::Command::new("go")
        .args(["env", "CGO_ENABLED"])
        .current_dir(dir)
        .output()
        .ok()
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
}

#[test]
#[ignore = "requires go (and a C toolchain) on PATH; run with cargo test -p guff-packages -- --ignored"]
fn cgo_test_variant_keeps_its_own_test_files() {
    if !go_available() {
        eprintln!("skipping: go not found on PATH");
        return;
    }
    let dir = testdata_dir();
    if !cgo_available(&dir) {
        eprintln!("skipping: CGO_ENABLED=0");
        return;
    }

    let cfg = Config {
        mode: LoadMode::LOAD_ALL_SYNTAX,
        dir: dir.clone(),
        tests: true,
        disable_cache: true,
        ..Config::default()
    };
    let pkgs = load(&cfg, &["./p".to_string()]).expect("load packages");

    let variant = pkgs
        .iter()
        .find(|p| p.id.contains(".test]") && !p.id.contains("_test "))
        .unwrap_or_else(|| {
            panic!(
                "no in-package test variant loaded; ids: {:?}",
                pkgs.iter().map(|p| &p.id).collect::<Vec<_>>()
            )
        });

    let has = |files: &[PathBuf], name: &str| {
        files
            .iter()
            .any(|f| f.file_name().is_some_and(|n| n == name))
    };

    // The variant is a cgo package, so it went through the second query …
    assert!(
        !variant.compiled_go_files.is_empty(),
        "the variant has no compiled files at all: {:?}",
        variant.id
    );
    // … and what came back has to be *its* file set.
    assert!(
        has(&variant.compiled_go_files, "p_test.go"),
        "the test variant lost its own test file; compiled: {:?}",
        variant.compiled_go_files
    );
    // `p.go` itself does not appear by name: it imports "C", so cgo rewrites
    // it and `go list -compiled` names the generated file under GOCACHE
    // instead. That rewriting is the entire reason this second query exists —
    // so what the variant must show is the generated files *plus* the test
    // file that cgo left alone.
    assert!(
        variant
            .compiled_go_files
            .iter()
            .any(|f| !f.starts_with(&dir)),
        "no cgo-generated file in the variant; compiled: {:?}",
        variant.compiled_go_files
    );

    // If the production package also survived the load (it is collapsed away
    // when a test variant exists — see `filter_duplicate_packages`), it must
    // not have gained the test file.
    if let Some(production) = pkgs.iter().find(|p| p.id == "example.com/cgotest/p") {
        assert!(
            !has(&production.compiled_go_files, "p_test.go"),
            "the production package gained a test file; compiled: {:?}",
            production.compiled_go_files
        );
    }
}
