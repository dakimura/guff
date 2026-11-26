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
    // Let inference pick the driver's hasher rather than naming it here.
    let export_paths = Default::default();
    let dep_graph = Default::default();
    typecheck_package(
        &mut pkg,
        &fset,
        &export_paths,
        &dep_graph,
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

/// golangci-lint de-duplicates diagnostics on `(position, analyzer, message)`
/// as it extracts them from the root actions (`pkg/goanalysis/runner.go`,
/// "De-duplicate diagnostics by position"). Without that step an analyzer that
/// legitimately reaches one line twice printed the finding twice where
/// golangci-lint printed it once: SA5003 reports a `defer` once per enclosing
/// infinite loop, so `for { for { defer f() } }` made two identical
/// diagnostics. The golden tier does see it — its diff is a multiset, not a
/// set — but only for a fixture that carries the shape.
///
/// The three keys that must survive are the three that differ from each other
/// in exactly one field: the position, the message, and nothing.
#[test]
fn duplicate_diagnostics_are_collapsed_on_analyzer_position_and_message() {
    fn run(pass: &mut guff_analysis::Pass<'_>) -> Result<Option<guff_analysis::AnalysisResult>, guff_analysis::RunError> {
        pass.reportf(1, "same message");
        pass.reportf(1, "same message"); // the duplicate
        pass.reportf(1, "other message"); // same position, different message
        pass.reportf(2, "same message"); // same message, different position
        Ok(None)
    }
    static A: std::sync::OnceLock<guff_analysis::Analyzer> = std::sync::OnceLock::new();
    let analyzer = A.get_or_init(|| guff_analysis::Analyzer {
        name: "dupes",
        doc: "reports the same thing twice",
        url: "",
        run: run as guff_analysis::RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    });

    let dir = smoke_dir("printast");
    let pkg = typecheck_fixture(&dir, "example.com/smoke/dupes");
    let result = run_on_packages(
        &[analyzer],
        std::slice::from_ref(&pkg),
        &RunnerOptions {
            sequential: true,
            ..RunnerOptions::default()
        },
    )
    .expect("run dupes");

    let mut got: Vec<(u32, String)> = result
        .diagnostics()
        .into_iter()
        .map(|(_, d)| (d.pos, d.message))
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            (1, "other message".to_string()),
            (1, "same message".to_string()),
            (2, "same message".to_string()),
        ],
        "{got:?}"
    );
}
