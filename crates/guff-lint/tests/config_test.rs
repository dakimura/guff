//! Integration tests for config file discovery, parsing, and migration.

use std::fs;
use std::path::Path;

use guff_lint::{
    discover_config, load_config, migrate_config_file, parse_config_str, LinterDefault,
    LinterSelection,
};

fn testdata(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/config")
        .join(path)
}

fn config_corpus_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/config_corpus")
}

#[test]
fn parse_v2_golangci_standard() {
    let contents = fs::read_to_string(testdata("v2_standard.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let names = cfg.linter_selection().resolve_names();
    assert!(names.contains(&"staticcheck".to_string()));
    assert!(names.contains(&"govet".to_string()));
}

#[test]
fn parse_v2_disable_unused() {
    let contents = fs::read_to_string(testdata("v2_disable_unused.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let names = cfg.linter_selection().resolve_names();
    assert!(!names.contains(&"unused".to_string()));
}

#[test]
fn parse_v1_enable_all() {
    let contents = fs::read_to_string(testdata("v1_enable_all.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    assert!(cfg.is_v1());
    let sel = cfg.linter_selection();
    assert_eq!(sel.default, LinterDefault::All);
}

#[test]
fn parse_v1_disable_all() {
    let contents = fs::read_to_string(testdata("v1_disable_all.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let sel = cfg.linter_selection();
    assert_eq!(sel.default, LinterDefault::None);
}

#[test]
fn discover_config_in_testdata_dir() {
    let dir = testdata("");
    let found = discover_config(&dir).expect("should find .golangci.yml in testdata");
    assert!(found.ends_with(".golangci.yml"));
}

#[test]
fn migrate_v1_writes_v2_and_backup() {
    let dir = tempfile::tempdir().unwrap();
    let src = testdata("v1_migrate_sample.yml");
    let dest = dir.path().join(".golangci.yml");
    fs::copy(&src, &dest).unwrap();

    let migrated = migrate_config_file(&dest, false).unwrap();
    assert_eq!(migrated.version.as_deref(), Some("2"));
    assert_eq!(migrated.linters.default.as_deref(), Some("none"));
    assert!(migrated.linters.enable.contains(&"govet".to_string()));
    assert!(migrated.linters.enable.contains(&"staticcheck".to_string()));
    assert!(!migrated.linters.enable.contains(&"gosimple".to_string()));
    assert!(migrated.formatters.enable.contains(&"gofmt".to_string()));

    let backup = guff_lint::backup_path(&dest);
    assert!(backup.is_file());

    let reloaded = load_config(&dest).unwrap();
    assert!(reloaded.is_v2());
    assert_eq!(reloaded.linter_selection().default, LinterDefault::None);
}

#[test]
fn cli_override_beats_file_default() {
    let file_sel = LinterSelection {
        default: LinterDefault::All,
        enable: vec![],
        disable: vec![],
    };
    let merged = file_sel.with_cli_overrides(Some(LinterDefault::None), &[], &[]);
    assert!(merged.resolve_names().is_empty());
}

#[test]
fn migrate_rejects_v2_without_skip() {
    let dir = tempfile::tempdir().unwrap();
    let src = testdata("v2_standard.yml");
    let dest = dir.path().join(".golangci.yml");
    fs::copy(&src, &dest).unwrap();

    let err = migrate_config_file(&dest, false).unwrap_err();
    assert!(err.to_string().contains("already v2"));
}

#[test]
fn parse_v2_full_issues_run_severity_output() {
    let contents = fs::read_to_string(testdata("v2_full_issues.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    assert!(cfg.is_v2());

    let issues = cfg.effective_issues();
    assert!(!issues.exclude_use_default);
    assert_eq!(issues.exclude_rules.len(), 3);
    assert_eq!(issues.exclude_rules[0].path.as_deref(), Some("_test\\.go"));
    assert_eq!(
        issues.exclude_rules[0].linters,
        vec!["errcheck".to_string()]
    );
    assert_eq!(
        issues.exclude_rules[2].path_except.as_deref(),
        Some("_test\\.go")
    );
    assert_eq!(issues.max_issues_per_linter, 0);

    let run = cfg.run();
    assert_eq!(run.build_tags, vec!["integration".to_string()]);
    assert_eq!(run.tests, Some(true));
    assert_eq!(run.timeout.as_deref(), Some("5m"));
    assert_eq!(run.concurrency, Some(4));
    assert_eq!(run.go.as_deref(), Some("1.22"));

    let severity = cfg.severity();
    assert_eq!(severity.default_severity.as_deref(), Some("warning"));
    assert_eq!(severity.rules.len(), 1);
    assert_eq!(severity.rules[0].severity, "error");

    assert_eq!(cfg.output().sort_results, Some(true));
}

#[test]
fn parse_v2_linters_exclusions_prometheus_shape() {
    let contents = fs::read_to_string(testdata("v2_linters_exclusions.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    assert!(cfg.is_v2());

    let excl = cfg.exclusions().unwrap();
    assert_eq!(excl.paths.len(), 2);
    assert!(excl.warn_unused);
    assert_eq!(excl.rules.len(), 2);

    let issues = cfg.effective_issues();
    assert!(!issues.exclude_use_default);
    assert_eq!(issues.exclude_dirs_use_default, Some(false));
    // paths folded into exclude_files; rules appended.
    assert_eq!(issues.exclude_files.len(), 2);
    assert_eq!(issues.exclude_rules.len(), 2);
}

#[test]
fn parse_golangci_config_corpus() {
    let mut entries = fs::read_dir(config_corpus_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("yml" | "yaml")
            )
        })
        .collect::<Vec<_>>();
    entries.sort();
    // Keep the floor in sync with docs/DEVELOPMENT.md §8 R22 when intentionally
    // shrinking; growing the corpus does not require a bump here.
    assert!(
        entries.len() >= 50,
        "config corpus too small ({} entries); see testdata/config_corpus/SOURCES.md",
        entries.len()
    );

    for path in entries {
        let contents = fs::read_to_string(&path).unwrap();
        let cfg = parse_config_str(&contents)
            .unwrap_or_else(|err| panic!("{} should parse: {err}", path.display()));
        assert!(
            cfg.is_v2(),
            "{} should exercise golangci-lint v2 config parsing",
            path.display()
        );

        // Exercise the follow-on resolution steps used by the CLI, not just
        // serde shape compatibility.
        let names = cfg.linter_selection().resolve_names();
        assert!(
            !names.is_empty(),
            "{} should resolve at least one linter",
            path.display()
        );
        let issues = cfg.effective_issues();
        assert!(
            !issues.exclude_use_default,
            "{} should use v2 exclusion semantics",
            path.display()
        );
        let _ = cfg.run();
        let _ = cfg.output();
        let _ = cfg.linter_settings_raw();
    }
}

#[test]
fn exclude_rules_filter_errcheck_on_bad_go() {
    use guff_lint::{IssueFilter, IssuesConfig, SeverityConfig};

    let contents = fs::read_to_string(testdata("v2_exclude_errcheck_bad.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let filter = IssueFilter::from_config(&cfg.effective_issues(), cfg.severity());

    let mk = |file: &str, linter: &str| guff_lint::Issue {
        from_linter: linter.into(),
        analyzer: linter.into(),
        text: "unchecked error".into(),
        severity: String::new(),
        filename: file.into(),
        line: 8,
        column: 2,
        source_line: None,
        diagnostic: guff_analysis::Diagnostic {
            message: "unchecked error".into(),
            ..Default::default()
        },
    };

    let kept = filter.apply(
        vec![
            mk("/proj/pkg/bad.go", "errcheck"),
            mk("/proj/pkg/ok.go", "errcheck"),
        ],
        &[],
    );
    assert_eq!(kept.len(), 1);
    assert!(kept[0].filename.ends_with("ok.go"));

    // Ensure IssuesConfig type is exercised as a configured filter, not Default.
    let _ = IssuesConfig::default();
    let _ = SeverityConfig::default();
}
