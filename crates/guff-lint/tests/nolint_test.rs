//! R3: `//nolint` suppresses findings; `nolintlint` reports unused directives.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use guff_lint::{
    analyzers_for_linter, IssueFilter, LintResult, SeverityConfig, IssuesConfig, NOLINTLINT_NAME,
};
use guff_packages::{typecheck_package, FxHashMap, LoadMode, Package, TypecheckEnv};
use guff_runner::{run_on_packages, RunnerOptions};
use guff_types::default_sizes;

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/run")
        .join(name)
}

fn typecheck_fixture(dir: &PathBuf, id: &str, file: &str) -> Arc<Package> {
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
        &FxHashMap::default(),
        &FxHashMap::default(),
        default_sizes(),
        &TypecheckEnv::default(),
        LoadMode::LOAD_SYNTAX,
    );
    Arc::new(pkg)
}

#[test]
fn nolint_errcheck_suppresses_finding() {
    let dir = fixture_dir("nolint_errcheck");
    let pkg = typecheck_fixture(&dir, "example.com/guff-test/nolint_errcheck", "bad.go");
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);

    let analyzers = analyzers_for_linter("errcheck").expect("errcheck");
    let filter = IssueFilter::from_config(
        &IssuesConfig {
            exclude_use_default: false,
            max_issues_per_linter: 0,
            max_same_issues: 0,
            exclude_dirs_use_default: Some(false),
            ..IssuesConfig::default()
        },
        &SeverityConfig::default(),
    );

    let result = LintResult {
        packages: vec![pkg.clone()],
        run: run_on_packages(
            &analyzers,
            std::slice::from_ref(&pkg),
            &RunnerOptions {
                sequential: true,
                ..RunnerOptions::default()
            },
        )
        .expect("run"),
        filter,
        cached_issues: Vec::new(),
        path_mode: guff_lint::PathMode::Rel,
        path_prefix: None,
    };

    assert!(
        result.raw_diagnostic_count() > 0,
        "fixture must produce errcheck raw findings before nolint"
    );
    assert_eq!(
        result.diagnostic_count(),
        0,
        "same-line //nolint:errcheck should suppress"
    );
}

#[test]
fn nolintlint_reports_unused_directive() {
    let dir = fixture_dir("nolint_unused");
    let pkg = typecheck_fixture(&dir, "example.com/guff-test/nolint_unused", "bad.go");
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);

    let analyzers = analyzers_for_linter("errcheck").expect("errcheck");
    let mut filter = IssueFilter::from_config(
        &IssuesConfig {
            exclude_use_default: false,
            max_issues_per_linter: 0,
            max_same_issues: 0,
            exclude_dirs_use_default: Some(false),
            ..IssuesConfig::default()
        },
        &SeverityConfig::default(),
    );
    filter.nolintlint = Some(guff_lint::NolintlintStyle {
        report_unused: true,
        ..guff_lint::NolintlintStyle::default()
    });

    let result = LintResult {
        packages: vec![pkg.clone()],
        run: run_on_packages(
            &analyzers,
            std::slice::from_ref(&pkg),
            &RunnerOptions {
                sequential: true,
                ..RunnerOptions::default()
            },
        )
        .expect("run"),
        filter,
        cached_issues: Vec::new(),
        path_mode: guff_lint::PathMode::Rel,
        path_prefix: None,
    };

    let issues = result.issues();
    assert!(
        issues.iter().any(|i| i.from_linter == NOLINTLINT_NAME),
        "expected unused nolintlint issue, got {issues:?}"
    );
}

#[test]
fn cli_honors_same_line_nolint() {
    let dir = fixture_dir("nolint_errcheck");
    let bin = env!("CARGO_BIN_EXE_guff");

    let out = Command::new(bin)
        .args([
            "run",
            "--no-config",
            "--enable",
            "errcheck",
            "--preset",
            "none",
            "--sequential",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn guff");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "expected exit 0 after nolint suppress; stdout={stdout} stderr={stderr}"
    );
    assert!(
        !stdout.contains("errcheck"),
        "nolinted finding must not appear on stdout: {stdout}"
    );
}

#[test]
fn cli_nolintlint_alone_reports_unused_directive() {
    let dir = fixture_dir("nolint_unused_bare");
    let bin = env!("CARGO_BIN_EXE_guff");

    let out = Command::new(bin)
        .args([
            "run",
            "--no-config",
            "--enable",
            "nolintlint",
            "--preset",
            "none",
            "--out-format",
            "json",
            "--sequential",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn guff");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("no analyzers enabled"),
        "nolintlint-only must still load packages: stderr={stderr}"
    );
    assert!(
        stdout.contains("nolintlint") && stdout.contains("unused"),
        "expected unused-directive JSON; stdout={stdout} stderr={stderr}"
    );
}
