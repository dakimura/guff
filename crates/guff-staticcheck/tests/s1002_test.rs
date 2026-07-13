//! Integration test for S1002 via the guff runner.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use guff_packages::{typecheck_package, LoadMode, Package, TypecheckEnv};
use guff_runner::{run_on_packages, RunnerOptions};
use guff_staticcheck::s1002;
use guff_types::default_sizes;

fn testdata() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/s1002")
}

fn typecheck_file(dir: &PathBuf, file: &str, id: &str) -> Arc<Package> {
    let mut pkg = Package {
        id: id.into(),
        pkg_path: id.into(),
        dir: dir.clone(),
        compiled_go_files: vec![dir.join(file)],
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
fn s1002_flags_redundant_bool_comparisons() {
    let dir = testdata();
    let pkg = typecheck_file(&dir, "bad.go", "example.com/staticcheck/s1002");
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);

    let result = run_on_packages(
        &[s1002::analyzer()],
        std::slice::from_ref(&pkg),
        &RunnerOptions {
            sequential: true,
            ..RunnerOptions::default()
        },
    )
    .expect("run S1002");

    let messages: Vec<_> = result
        .diagnostics()
        .into_iter()
        .map(|(_, d)| d.message)
        .collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("simplified to x")));
    assert!(messages.iter().any(|m| m.contains("simplified to !x")));
}

#[test]
fn s1002_allows_valid_comparisons() {
    let dir = testdata();
    let pkg = typecheck_file(&dir, "ok.go", "example.com/staticcheck/s1002/ok");
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);

    let result = run_on_packages(
        &[s1002::analyzer()],
        std::slice::from_ref(&pkg),
        &RunnerOptions {
            sequential: true,
            ..RunnerOptions::default()
        },
    )
    .expect("run S1002");

    assert!(result.diagnostics().is_empty());
}
