//! R1: diagnostics go to stdout; `--issues-exit-code` controls the success exit code.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use guff_lint::{analyzers_for_linter, LintResult};
use guff_packages::{typecheck_package, LoadMode, Package, TypecheckEnv};
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
        &HashMap::new(),
        &HashMap::new(),
        default_sizes(),
        &TypecheckEnv::default(),
        LoadMode::LOAD_SYNTAX,
    );
    Arc::new(pkg)
}

fn lint_errcheck_fixture() -> LintResult {
    let dir = fixture_dir("unchecked_error");
    let pkg = typecheck_fixture(&dir, "example.com/guff-test/unchecked_error", "bad.go");
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);

    let analyzers = analyzers_for_linter("errcheck").expect("errcheck registered");
    let run = run_on_packages(
        &analyzers,
        std::slice::from_ref(&pkg),
        &RunnerOptions {
            sequential: true,
            ..RunnerOptions::default()
        },
    )
    .expect("run errcheck");

    LintResult {
        packages: vec![pkg],
        run,
        filter: guff_lint::IssueFilter::default(),
        cached_issues: Vec::new(),
        path_mode: guff_lint::PathMode::Rel,
        path_prefix: None,
    }
}

#[test]
fn print_text_writes_diagnostics_to_writer() {
    let result = lint_errcheck_fixture();
    assert!(result.diagnostic_count() > 0);

    let mut buf = Vec::new();
    let n = result.print_text(&mut buf).expect("print");
    assert_eq!(n, result.diagnostic_count());

    let text = String::from_utf8(buf).expect("utf8");
    assert!(
        text.contains("errcheck") || text.contains("error"),
        "unexpected text: {text}"
    );
    assert!(text.contains("bad.go"), "expected path in stdout-style text: {text}");
}

#[test]
fn exit_code_uses_issues_exit_code_when_findings() {
    let result = lint_errcheck_fixture();
    assert!(result.diagnostic_count() > 0);
    assert_eq!(result.exit_code(1), 1);
    assert_eq!(result.exit_code(0), 0);
    assert_eq!(result.exit_code(42), 42);
}

#[test]
fn exit_code_is_zero_when_clean() {
    let dir = fixture_dir("unchecked_error");
    // Re-typecheck the same package but run no analyzers → no diagnostics.
    let pkg = typecheck_fixture(&dir, "example.com/guff-test/unchecked_error", "bad.go");
    let run = run_on_packages(
        &[],
        std::slice::from_ref(&pkg),
        &RunnerOptions {
            sequential: true,
            ..RunnerOptions::default()
        },
    )
    .expect("empty analyzer set");
    let result = LintResult {
        packages: vec![pkg],
        run,
        filter: guff_lint::IssueFilter::default(),
        cached_issues: Vec::new(),
        path_mode: guff_lint::PathMode::Rel,
        path_prefix: None,
    };
    assert_eq!(result.diagnostic_count(), 0);
    assert_eq!(result.exit_code(1), 0);
    assert_eq!(result.exit_code(42), 0);
}

#[test]
fn cli_writes_issues_to_stdout_and_honors_issues_exit_code() {
    let dir = fixture_dir("unchecked_error");
    let bin = env!("CARGO_BIN_EXE_guff");

    let with_findings = Command::new(bin)
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

    let stdout = String::from_utf8_lossy(&with_findings.stdout);
    let stderr = String::from_utf8_lossy(&with_findings.stderr);
    assert!(
        with_findings.status.success(),
        "expected exit 0 with --issues-exit-code 0; status={:?}\nstdout={stdout}\nstderr={stderr}",
        with_findings.status.code()
    );
    assert!(
        !stdout.trim().is_empty(),
        "diagnostics must appear on stdout; stderr={stderr}"
    );
    assert!(
        stdout.contains("bad.go") || stdout.contains("errcheck") || stdout.contains("error"),
        "unexpected stdout: {stdout}"
    );
    // Diagnostics must not be on stderr (status / unknown-linter messages OK).
    assert!(
        !stderr.contains("bad.go"),
        "diagnostics leaked to stderr: {stderr}"
    );

    let default_exit = Command::new(bin)
        .args([
            "run",
            "--no-config",
            "--enable",
            "errcheck",
            "--sequential",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn guff");

    assert_eq!(
        default_exit.status.code(),
        Some(1),
        "default issues exit code should be 1\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&default_exit.stdout),
        String::from_utf8_lossy(&default_exit.stderr)
    );
    assert!(!default_exit.stdout.is_empty());
}
