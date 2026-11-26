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

/// Every exclusion runs *before* the `//nolint` processor upstream, so a
/// finding an exclusion removes never reaches a directive and the directive is
/// left unused.
///
/// guff marked directives first, with a comment claiming that a directive
/// covering an excluded finding still counts as used. It does not — measured
/// with an `exclusions.rules` entry matching `source: Rollback` (syncthing,
/// five directives) and with the `std-error-handling` preset's EXC0001 over
/// `defer f.Close()`.
///
/// The second directive is the control: nothing excludes *its* finding, so it
/// stays used and nolintlint says nothing about it.
#[test]
fn an_excluded_finding_leaves_its_nolint_directive_unused() {
    let dir = fixture_dir("nolint_excluded");
    let pkg = typecheck_fixture(&dir, "example.com/guff-test/nolint_excluded", "bad.go");
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);

    let analyzers = analyzers_for_linter("errcheck").expect("errcheck");
    let mut filter = IssueFilter::from_config(
        &IssuesConfig {
            exclude_use_default: false,
            max_issues_per_linter: 0,
            max_same_issues: 0,
            exclude_dirs_use_default: Some(false),
            exclude_rules: vec![guff_lint::ExcludeRule {
                linters: vec!["errcheck".into()],
                source: Some("Rollback".into()),
                ..guff_lint::ExcludeRule::default()
            }],
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
    let unused: Vec<&str> = issues
        .iter()
        .filter(|i| i.from_linter == NOLINTLINT_NAME)
        .map(|i| i.text.as_str())
        .collect();
    assert_eq!(
        unused,
        vec!["directive `//nolint:errcheck` is unused for linter \"errcheck\""],
        "{issues:?}"
    );
}

/// A directive that suppressed nothing, inside the range of one that did.
///
/// nolintlint emits an unused *candidate* for every directive and the nolint
/// filter cancels the used ones — through the same range loop every issue takes,
/// so any covering range that matched something cancels the candidate, not only
/// the range the directive itself created. Here the file-level
/// `//nolint:errcheck` really does suppress the `mkerr()` finding, and that
/// silences the unrelated `//nolint` further down as a side effect.
///
/// Found by compat/fuzz.py: no hand-written fixture had two directives whose
/// ranges overlapped, and with one directive per range the wrong reading ("did
/// *my* directive suppress anything") gives the right answer every time.
#[test]
fn nolintlint_unused_is_cancelled_by_a_covering_directive_that_matched() {
    let dir = fixture_dir("nolint_unused_covered");
    let pkg = typecheck_fixture(&dir, "example.com/guff-test/nolint_unused_covered", "bad.go");
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

    let nolintlint: Vec<_> = result
        .issues()
        .into_iter()
        .filter(|i| i.from_linter == NOLINTLINT_NAME)
        .collect();
    assert!(
        nolintlint.is_empty(),
        "both directives are cancelled by the file-level range: {nolintlint:?}"
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
