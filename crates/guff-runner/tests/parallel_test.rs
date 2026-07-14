//! R9: sequential vs parallel action DAG must produce identical diagnostics.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use guff_packages::{typecheck_package, LoadMode, Package, TypecheckEnv};
use guff_runner::{run_on_packages, RunnerOptions};
use guff_types::default_sizes;

fn fixture_pkg(rel: &str, id: &str) -> Arc<Package> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/smoke")
        .join(rel);
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

fn diag_keys(diags: &[(String, guff_analysis::Diagnostic)]) -> Vec<(String, u32, String)> {
    let mut keys: Vec<_> = diags
        .iter()
        .map(|(action_id, d)| (action_id.clone(), d.pos, d.message.clone()))
        .collect();
    keys.sort();
    keys
}

#[test]
fn sequential_and_parallel_diagnostics_match() {
    let printast = fixture_pkg("printast", "example.com/smoke/printast");
    let printf = fixture_pkg("printf", "example.com/smoke/printf");
    assert!(!printast.ill_typed, "{:?}", printast.errors);
    assert!(!printf.ill_typed, "{:?}", printf.errors);

    let analyzers = [
        guff_analysis::passes::printast_analyzer(),
        guff_analysis::passes::printf_analyzer(),
    ];
    let packages = [printast, printf];

    let seq = run_on_packages(
        &analyzers,
        &packages,
        &RunnerOptions {
            sequential: true,
            ..RunnerOptions::default()
        },
    )
    .expect("sequential");

    let par = run_on_packages(
        &analyzers,
        &packages,
        &RunnerOptions {
            sequential: false,
            concurrency: Some(4),
            ..RunnerOptions::default()
        },
    )
    .expect("parallel");

    assert_eq!(
        diag_keys(&seq.diagnostics()),
        diag_keys(&par.diagnostics()),
        "parallel diagnostics must match sequential"
    );
}

#[test]
fn package_is_sync_after_ident_mutex() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<Package>();
}
