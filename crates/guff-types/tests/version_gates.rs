//! Chunk-56 tests: builtin version gates wired through
//! `Checker::verify_versionf` (chunk 55's `version.rs`). A file's effective Go
//! version (`file.go_version`) flows through the resolver into `env.version`;
//! builtins predating that version report `UnsupportedFeature`. An empty
//! version disables the gates (Go's unset behaviour).

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_types::{Checker, Config};
use guff_types_errors::Code;

/// Parse `src`, stamp it with the given effective Go `version`, and type-check.
fn check_versioned(src: &str, version: &str) -> Checker {
    let fset = FileSet::new();
    let mut file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
    file.go_version = version.to_string();
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file]);
    check
}

fn has_unsupported(check: &Checker) -> bool {
    check
        .errors
        .iter()
        .any(|e| e.code == Code::UnsupportedFeature)
}

const MIN_SRC: &str = "package p\nfunc f() { _ = min(1, 2) }\n";
const CLEAR_SRC: &str = "package p\nfunc f(m map[int]int) { clear(m) }\n";
const ADD_SRC: &str =
    "package p\nimport \"unsafe\"\nfunc f(p unsafe.Pointer) { _ = unsafe.Add(p, 1) }\n";

#[test]
fn file_version_is_recorded_in_info() {
    let fset = FileSet::new();
    let mut file = parse_file(&fset, "test.go", MIN_SRC.as_bytes(), Mode::NONE).expect("parse");
    file.go_version = "go1.21".to_string();
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    assert_eq!(
        check.info.file_versions.get(&file.id),
        Some(&"go1.21".to_string())
    );
}

// ----------------------------------------------------------- min/max (go1.21)

#[test]
fn min_requires_go121() {
    let c = check_versioned(MIN_SRC, "go1.16");
    assert!(
        has_unsupported(&c),
        "expected version error, got: {:?}",
        c.errors
    );
}

#[test]
fn min_allowed_at_go121() {
    let c = check_versioned(MIN_SRC, "go1.21");
    assert!(
        !has_unsupported(&c),
        "unexpected version error: {:?}",
        c.errors
    );
}

#[test]
fn min_allowed_when_version_unset() {
    // An empty effective version disables version checks.
    let c = check_versioned(MIN_SRC, "");
    assert!(
        !has_unsupported(&c),
        "unexpected version error: {:?}",
        c.errors
    );
}

// -------------------------------------------------------------- clear (go1.21)

#[test]
fn clear_requires_go121() {
    let c = check_versioned(CLEAR_SRC, "go1.16");
    assert!(
        has_unsupported(&c),
        "expected version error, got: {:?}",
        c.errors
    );
}

#[test]
fn clear_allowed_at_go121() {
    let c = check_versioned(CLEAR_SRC, "go1.21");
    assert!(
        !has_unsupported(&c),
        "unexpected version error: {:?}",
        c.errors
    );
}

// --------------------------------------------------------- unsafe.Add (go1.17)

#[test]
fn unsafe_add_requires_go117() {
    let c = check_versioned(ADD_SRC, "go1.16");
    assert!(
        has_unsupported(&c),
        "expected version error, got: {:?}",
        c.errors
    );
}

#[test]
fn unsafe_add_allowed_at_go117() {
    let c = check_versioned(ADD_SRC, "go1.17");
    assert!(
        !has_unsupported(&c),
        "unexpected version error: {:?}",
        c.errors
    );
}
