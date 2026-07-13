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
    assert_eq!(
        reloaded.linter_selection().default,
        LinterDefault::None
    );
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
