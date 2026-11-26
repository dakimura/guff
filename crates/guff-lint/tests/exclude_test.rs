//! R2: exclude-rules suppress findings from real analyzer output.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use guff_lint::{
    analyzers_for_linter, load_config, IssueFilter, LintResult, IssuesConfig, SeverityConfig,
};
use guff_packages::{typecheck_package, FxHashMap, LoadMode, Package, TypecheckEnv};
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
        &FxHashMap::default(),
        &FxHashMap::default(),
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
    let filter = IssueFilter::from_config(&cfg.effective_issues(), cfg.severity());

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
        cached_issues: Vec::new(),
        path_mode: guff_lint::PathMode::Rel,
        path_prefix: None,
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
        cached_issues: Vec::new(),
        path_mode: guff_lint::PathMode::Rel,
        path_prefix: None,
    };
    assert!(result.diagnostic_count() > 0);
}

#[test]
fn v2_linters_exclusions_paths_and_rules() {
    let cfg = load_config(&config_path("v2_linters_exclusions.yml")).unwrap();
    let excl = cfg.exclusions().expect("v2 exclusions");
    assert_eq!(excl.paths.len(), 2);
    assert_eq!(excl.rules.len(), 2);
    assert!(excl.warn_unused);

    let issues = cfg.effective_issues();
    assert!(!issues.exclude_use_default);
    assert_eq!(issues.exclude_dirs_use_default, Some(false));
    assert!(issues
        .exclude_files
        .iter()
        .any(|p| p.contains(r"\.(l|pb|y)\.go")));
    assert!(issues.exclude_rules.iter().any(|r| {
        r.path.as_deref() == Some("bad\\.go") && r.linters.iter().any(|l| l == "errcheck")
    }));

    let filter = IssueFilter::from_config(&issues, cfg.severity());
    let mk = |file: &str, text: &str| guff_lint::Issue {
        from_linter: "errcheck".into(),
        analyzer: "errcheck".into(),
        text: text.into(),
        severity: String::new(),
        filename: file.into(),
        line: 8,
        column: 2,
        source_line: None,
        diagnostic: guff_analysis::Diagnostic {
            message: text.into(),
            ..Default::default()
        },
    };

    let kept = filter.apply(
        vec![
            mk("/proj/pkg/bad.go", "unchecked error"),
            mk("/proj/pkg/ok.go", "unchecked error"),
            mk("/proj/pkg/foo.pb.go", "unchecked error"),
            mk(
                "/proj/pkg/ok.go",
                "Error return value of resp.Body.Close is not checked",
            ),
        ],
        &[],
    );
    assert_eq!(
        kept.len(),
        1,
        "path rule + path glob + text rule should leave only ok.go unchecked: {kept:?}"
    );
    assert!(kept[0].filename.ends_with("ok.go"));
    assert_eq!(kept[0].text, "unchecked error");
}

#[test]
fn v2_linters_exclusions_presets_expand() {
    let cfg = load_config(&config_path("v2_linters_exclusions_presets.yml")).unwrap();
    let issues = cfg.effective_issues();
    assert!(!issues.exclude_use_default);
    assert!(
        issues.exclude_rules.len() >= 2,
        "presets should inject rules, got {}",
        issues.exclude_rules.len()
    );
    assert!(issues.exclude_rules.iter().any(|r| {
        r.linters.iter().any(|l| l == "errcheck")
            && r.text
                .as_deref()
                .is_some_and(|t| t.contains("Close") || t.contains("std"))
    }));
    assert!(issues
        .exclude_rules
        .iter()
        .any(|r| r.linters.iter().any(|l| l == "revive")));
}

#[test]
fn cli_honors_v2_linters_exclusions() {
    let dir = fixture_dir("unchecked_error");
    let cfg = config_path("v2_linters_exclusions.yml");
    let bin = env!("CARGO_BIN_EXE_guff");

    let out = Command::new(bin)
        .args(["run", "-c", cfg.to_str().unwrap(), "--sequential", "."])
        .current_dir(&dir)
        .output()
        .expect("spawn guff");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "linters.exclusions path=bad.go should yield exit 0\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(
        !stdout.contains("bad.go"),
        "excluded path must not appear on stdout: {stdout}"
    );
}
