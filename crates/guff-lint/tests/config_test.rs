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

    // v2's key is `severity.default`; the post-processor reads it through
    // `effective_severity`, which maps it onto the v1-named field.
    let severity = cfg.severity();
    assert_eq!(severity.default_v2.as_deref(), Some("warning"));
    assert_eq!(severity.default_severity, None);
    assert_eq!(
        cfg.effective_severity().default_severity.as_deref(),
        Some("warning")
    );
    assert_eq!(severity.rules.len(), 1);
    assert_eq!(severity.rules[0].severity, "error");

    // `output.sort-results` is v1's key (v2 renamed it `sort-order` and made it
    // a list), so a v2 file cannot set it however it spells it.
    assert_eq!(cfg.output().sort_results, None);
    assert_eq!(cfg.output().path_prefix.as_deref(), Some(""));
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
    // `linters.exclusions.generated` defaults to strict: golangci's config
    // loader fills the empty value in with `GeneratedModeStrict` before any
    // processor runs. Only `formatters.exclusions.generated` is left empty,
    // and empty means lax there. See cases/generated{,-lax}.
    assert_eq!(issues.generated.as_deref(), Some("strict"));
}

#[test]
fn parse_new_from_merge_base_and_whole_files() {
    let yaml = r#"
version: "2"
linters:
  default: none
  enable: [errcheck]
  exclusions:
    generated: strict
issues:
  new-from-merge-base: origin/main
  whole-files: true
  max-issues-per-linter: 0
  max-same-issues: 0
"#;
    let cfg = parse_config_str(yaml).unwrap();
    let issues = cfg.effective_issues();
    assert_eq!(issues.new_from_merge_base.as_deref(), Some("origin/main"));
    assert!(issues.whole_files);
    assert_eq!(issues.generated.as_deref(), Some("strict"));
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

        // These are configs their projects run golangci-lint with every day, so
        // upstream's validator accepts all of them by construction. That makes
        // the corpus the standing guard on the other side of
        // `ConfigFile::validate`: a rule ported too strictly (a preset spelling,
        // a condition count) shows up here as a real repo guff would refuse.
        cfg.validate()
            .unwrap_or_else(|err| panic!("{} should validate: {err}", path.display()));

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
fn output_print_flags_parse_into_printer_options() {
    use guff_lint::PrinterOptions;

    // No `version:` line: `print-issued-lines` / `print-linter-name` are v1's
    // spelling of these two, and v2 moved them under `output.formats.text`.
    let contents = r#"
output:
  print-issued-lines: false
  print-linter-name: false
"#;
    let cfg = parse_config_str(contents).unwrap();
    let out = cfg.output();
    assert_eq!(out.print_issued_lines, Some(false));
    assert_eq!(out.print_linter_name, Some(false));
    let opts = PrinterOptions::from_config(out.print_issued_lines, out.print_linter_name);
    assert!(!opts.print_issued_lines);
    assert!(!opts.print_linter_name);

    let defaults = PrinterOptions::from_config(None, None);
    assert!(defaults.print_issued_lines);
    assert!(defaults.print_linter_name);
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

// ---------------------------------------------------------------------------
// v1 keys inside a v2 file (COMPAT-HARDENING §4, hashicorp/packer)
// ---------------------------------------------------------------------------
//
// golangci-lint decides "this is v2" from the `version` scalar alone and then
// decodes the body with viper/mapstructure, which drops every key its v2
// structs do not declare. A v1 config with a v2 version line therefore runs
// with almost none of itself in effect. These tests fix both halves of that:
// which scalars count as version 2, and which keys survive the decode.

/// The four spellings upstream reads as version 2.
///
/// viper decodes with `WeaklyTypedInput`, so the numbers reach
/// `Loader.checkConfigurationVersion` as the string `"2"`. packer writes the
/// unquoted integer.
#[test]
fn version_two_is_recognized_however_it_is_spelled() {
    let spellings = ["\"2\"", "'2'", "2", "2.0"];
    for spelling in spellings {
        let cfg = parse_config_str(&format!(
            "version: {spelling}\nlinters:\n  default: none\n  enable: [errcheck]\n"
        ))
        .unwrap();
        assert!(cfg.is_v2(), "version: {spelling} should parse as v2");
    }

    // Anything else is not version 2. golangci-lint refuses to start on these;
    // guff still reads them as v1 (it supports v1 configs on purpose), so the
    // assertion here is only that they are *not* v2.
    for spelling in ["1", "\"1\"", "two", "2.5"] {
        let cfg = parse_config_str(&format!(
            "version: {spelling}\nlinters:\n  default: none\n  enable: [errcheck]\n"
        ))
        .unwrap();
        assert!(cfg.is_v1(), "version: {spelling} should not parse as v2");
    }
}

const V1_ISSUES_BODY: &str = "\
issues:
  exclude:
    - 'Error return value'
  exclude-rules:
    - linters: [errcheck]
      path: '_test\\.go'
  exclude-dirs:
    - vendored
  exclude-files:
    - 'zz_generated\\.go'
  exclude-use-default: true
  exclude-case-sensitive: true
  exclude-dirs-use-default: true
  include:
    - EXC0001
  max-issues-per-linter: 0
  max-same-issues: 0
";

/// Every v1 `issues` key is invisible in a v2 file — and the v2 keys next to
/// them are not.
#[test]
fn v1_issues_keys_are_dropped_from_a_v2_config() {
    let cfg = parse_config_str(&format!(
        "version: 2\nlinters:\n  default: none\n  enable: [errcheck]\n{V1_ISSUES_BODY}"
    ))
    .unwrap();
    assert!(cfg.is_v2());

    let issues = cfg.issues();
    assert_eq!(issues.exclude.len(), 0);
    assert_eq!(issues.exclude_rules.len(), 0);
    assert_eq!(issues.exclude_dirs.len(), 0);
    assert_eq!(issues.exclude_files.len(), 0);
    assert_eq!(issues.include.len(), 0);
    // The three bools land on serde's defaults because the keys never reached
    // the struct — not on the values the file asked for.
    assert!(issues.exclude_use_default);
    assert!(!issues.exclude_case_sensitive);
    assert_eq!(issues.exclude_dirs_use_default, None);
    // …while the keys v2 *does* declare are read as written.
    assert_eq!(issues.max_issues_per_linter, 0);
    assert_eq!(issues.max_same_issues, 0);

    // And what the filter finally sees is v2's own answer for all three: no
    // default exclusions, case-sensitive patterns, no default dirs.
    let effective = cfg.effective_issues();
    assert!(!effective.exclude_use_default);
    assert!(effective.exclude_case_sensitive);
    assert_eq!(effective.exclude_dirs_use_default, Some(false));
    assert_eq!(effective.exclude_rules.len(), 0);
}

/// The same body under a v1 file still works. A whitelist that over-reaches
/// would look exactly like the fix, and only this test would notice.
#[test]
fn v1_issues_keys_still_work_in_a_v1_config() {
    let cfg = parse_config_str(&format!(
        "linters:\n  disable-all: true\n  enable: [errcheck]\n{V1_ISSUES_BODY}"
    ))
    .unwrap();
    assert!(cfg.is_v1());

    let issues = cfg.issues();
    assert_eq!(issues.exclude.len(), 1);
    assert_eq!(issues.exclude_rules.len(), 1);
    assert_eq!(issues.exclude_dirs.len(), 1);
    assert_eq!(issues.exclude_files.len(), 1);
    assert_eq!(issues.include.len(), 1);
    assert_eq!(issues.exclude_dirs_use_default, Some(true));
    assert_eq!(issues.max_issues_per_linter, 0);
}

/// v2's `linters.exclusions` is the section that replaced them, and it must
/// still reach the filter through `effective_issues`.
#[test]
fn v2_exclusions_survive_the_v1_key_drop() {
    let cfg = parse_config_str(
        "version: 2
linters:
  default: none
  enable: [errcheck]
  exclusions:
    paths:
      - vendored
    rules:
      - linters: [errcheck]
        path: '_test\\.go'
issues:
  exclude-rules:
    - linters: [errcheck]
      path: 'never\\.go'
",
    )
    .unwrap();

    let effective = cfg.effective_issues();
    assert_eq!(effective.exclude_rules.len(), 1);
    assert_eq!(
        effective.exclude_rules[0].path.as_deref(),
        Some("_test\\.go")
    );
    assert_eq!(effective.exclude_files.len(), 1);
    assert_eq!(effective.exclude_files[0], "vendored");
}

/// v1's `output` keys moved into `output.formats.<name>` (or vanished), and the
/// v1 format *names* are not among v2's nine.
#[test]
fn v1_output_keys_are_dropped_from_a_v2_config() {
    let body = "\
output:
  print-issued-lines: false
  print-linter-name: false
  sort-results: true
  format: tab
  path-prefix: pre
  formats:
    colored-line-number:
      path: stdout
    line-number:
      path: stdout
    tab:
      path: stdout
";
    let cfg = parse_config_str(&format!(
        "version: 2\nlinters:\n  default: none\n  enable: [errcheck]\n{body}"
    ))
    .unwrap();

    let output = cfg.output();
    assert_eq!(output.print_issued_lines, None);
    assert_eq!(output.print_linter_name, None);
    assert_eq!(output.sort_results, None);
    assert_eq!(output.format, None);
    // v2 declares `path-prefix`, so it stays.
    assert_eq!(output.path_prefix.as_deref(), Some("pre"));

    let formats = output.formats.as_mapping().expect("formats is a mapping");
    let names: Vec<&str> = formats.keys().filter_map(|k| k.as_str()).collect();
    assert_eq!(names, vec!["tab"]);

    // The v1 file keeps all of it.
    let v1 = parse_config_str(&format!("linters:\n  enable: [errcheck]\n{body}")).unwrap();
    assert!(v1.is_v1());
    assert_eq!(v1.output().print_linter_name, Some(false));
    assert_eq!(v1.output().format.as_deref(), Some("tab"));
    assert_eq!(
        v1.output()
            .formats
            .as_mapping()
            .expect("formats is a mapping")
            .len(),
        3
    );
}

/// v1's `linters.disable-all` / `fast` / `presets`, the top-level
/// `linters-settings`, and `run.skip-*` fall out of guff's own structs the same
/// way mapstructure drops them — no whitelist needed. Asserted so a later
/// refactor that adds one of those fields back has to notice.
#[test]
fn other_v1_sections_are_invisible_in_a_v2_config() {
    let cfg = parse_config_str(
        "version: 2
linters:
  disable-all: true
  fast: true
  presets: [bugs]
  enable: [errcheck]
linters-settings:
  errcheck:
    check-blank: true
run:
  skip-files:
    - '.*_test\\.go'
  skip-dirs-use-default: true
",
    )
    .unwrap();

    // `disable-all` did not become `default: none`: with no `linters.default`,
    // v2 starts from the standard set and `enable` adds to it.
    let names = cfg.linter_selection().resolve_names();
    assert!(names.contains(&"errcheck".to_string()));
    assert!(names.contains(&"staticcheck".to_string()));
    assert!(cfg.linter_settings_raw().is_null());
    assert_eq!(cfg.run().build_tags.len(), 0);
}
