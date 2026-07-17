//! End-to-end smoke tests: packages → typecheck → runner → diagnostics.
//!
//! Uses `typecheck_package` + `run_on_packages` so CI does not require `go` on
//! PATH. Full `go list` → `load` → `run` is covered by `guff-packages` integration
//! tests; when `go` is missing, [`guff_packages::OfflineDriver`] (PL02 / R20)
//! provides the same metadata path without a toolchain.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use guff_packages::{typecheck_package, LoadMode, Package, TypecheckEnv};
use guff_runner::{run_on_packages, RunnerOptions};
use guff_types::default_sizes;

fn smoke_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/smoke")
        .join(name)
}

fn typecheck_fixture(dir: &PathBuf, id: &str) -> Arc<Package> {
    let mut pkg = Package {
        id: id.into(),
        pkg_path: id.into(),
        dir: dir.clone(),
        compiled_go_files: vec![dir.join("main.go")],
        ..Package::default()
    };
    let fset = guff::position::FileSet::new();
    typecheck_package(
        &mut pkg,
        &fset,
        &HashMap::new(),
        &HashMap::new(),
        default_sizes(),
        &TypecheckEnv::default(),
        LoadMode::LOAD_SYNTAX,
    );
    Arc::new(pkg)
}

#[test]
fn smoke_printast_reports_first_func() {
    let dir = smoke_dir("printast");
    let pkg = typecheck_fixture(&dir, "example.com/smoke/printast");
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);

    let result = run_on_packages(
        &[guff_analysis::passes::printast_analyzer()],
        std::slice::from_ref(&pkg),
        &RunnerOptions {
            sequential: true,
            ..RunnerOptions::default()
        },
    )
    .expect("run printast");

    let diags = result.diagnostics();
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert!(
        diags[0].1.message.contains("printast: found FuncDecl Hello"),
        "{}",
        diags[0].1.message
    );
}

#[test]
fn smoke_printf_flags_bad_verb() {
    let dir = smoke_dir("printf");
    let pkg = typecheck_fixture(&dir, "example.com/smoke/printf");
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);

    let result = run_on_packages(
        &[guff_analysis::passes::printf_analyzer()],
        std::slice::from_ref(&pkg),
        &RunnerOptions {
            sequential: true,
            ..RunnerOptions::default()
        },
    )
    .expect("run printf");

    let diags = result.diagnostics();
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {diags:?}");
    assert!(
        diags[0].1.message.contains("unknown verb %z"),
        "{}",
        diags[0].1.message
    );
}
