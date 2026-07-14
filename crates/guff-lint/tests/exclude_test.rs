//! R2: exclude-rules suppress findings from real analyzer output.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use guff_lint::{
    analyzers_for_linter, load_config, IssueFilter, LintResult, IssuesConfig, SeverityConfig,
};
use guff_packages::{typecheck_package, LoadMode, Package, TypecheckEnv};
use guff_runner::{run_on_packages, RunnerOptions};
use guff_types::default_sizes;

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/run")
        .join(name)
}

fn config_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/config")
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
        &HashMap::new(),
        &HashMap::new(),
        default_sizes(),
        &TypecheckEnv::default(),
        LoadMode::LOAD_SYNTAX,
    );
    Arc::new(pkg)
}

#[test]
fn exclude_rules_drop_errcheck_on_bad_go() {
    let dir = fixture_dir("unchecked_error");
    let pkg = typecheck_fixture(&dir, "example.com/guff-test/unchecked_error", "bad.go");
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);

    let analyzers = analyzers_for_linter("errcheck").expect("errcheck");
    let cfg = load_config(&config_path("v2_exclude_errcheck_bad.yml")).unwrap();
    let filter = IssueFilter::from_config(cfg.issues(), cfg.severity());

    let with_filter = LintResult {
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
    };
    assert!(
        with_filter.raw_diagnostic_count() > 0,
        "fixture must produce errcheck findings"
    );
    assert_eq!(
        with_filter.diagnostic_count(),
        0,
        "exclude-rules path=bad.go should drop all errcheck findings"
    );
    assert_eq!(with_filter.exit_code(1), 0);
}

#[test]
fn cli_honors_exclude_rules_from_config() {
    let dir = fixture_dir("unchecked_error");
    let cfg = config_path("v2_exclude_errcheck_bad.yml");
    let bin = env!("CARGO_BIN_EXE_guff");

    let out = Command::new(bin)
        .args([
            "run",
            "-c",
            cfg.to_str().unwrap(),
            "--sequential",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn guff");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "exclude-rules should yield exit 0\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(
        !stdout.contains("bad.go"),
        "excluded path must not appear on stdout: {stdout}"
    );

    // Without the exclude config, findings remain.
    let raw = Command::new(bin)
        .args([
            "run",
            "--no-config",
            "--enable",
            "errcheck",
            "--sequential",
            "--issues-exit-code",
            "0",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn guff");
    let raw_out = String::from_utf8_lossy(&raw.stdout);
    assert!(
        raw_out.contains("bad.go") || raw_out.contains("errcheck") || raw_out.contains("error"),
        "control run must report findings: {raw_out}"
    );
}

#[test]
fn default_filter_is_noop_for_library_use() {
    let _ = IssuesConfig::default();
    let _ = SeverityConfig::default();
    let filter = IssueFilter::default();
    let dir = fixture_dir("unchecked_error");
    let pkg = typecheck_fixture(&dir, "example.com/guff-test/unchecked_error", "bad.go");
    let analyzers = analyzers_for_linter("errcheck").unwrap();
    let result = LintResult {
        packages: vec![pkg.clone()],
        run: run_on_packages(
            &analyzers,
            std::slice::from_ref(&pkg),
            &RunnerOptions {
                sequential: true,
                ..Default::default()
            },
        )
        .unwrap(),
        filter,
    };
    assert!(result.diagnostic_count() > 0);
}
