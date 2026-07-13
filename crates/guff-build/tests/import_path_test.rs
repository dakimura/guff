//! Tests for module-mode import path resolution.

use std::path::PathBuf;

use guff_build::{parse_mod_contents, Context, ImportMode, ModFile};

fn testdata(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata")
        .join(path)
}

#[test]
fn parse_mod_file_extracts_module_and_requires() {
    let data = std::fs::read_to_string(testdata("module/go.mod")).unwrap();
    let m = parse_mod_contents(&data).unwrap();
    assert_eq!(
        m,
        ModFile {
            module_path: "example.com/mod".to_string(),
            go_version: Some("1.21".to_string()),
            requires: vec![guff_build::Require {
                path: "example.com/other".to_string(),
                version: "v1.0.0".to_string(),
            }],
        }
    );
}

#[test]
fn import_resolves_subpackage_in_module() {
    let src = testdata("module");
    let pkg = Context::default()
        .import("example.com/mod/pkg/sub", &src, ImportMode::NONE)
        .unwrap();
    assert_eq!(pkg.name, "sub");
    assert_eq!(pkg.go_files, vec!["sub.go"]);
    assert_eq!(pkg.import_path, "example.com/mod/pkg/sub");
}

#[test]
fn import_resolves_module_root_package() {
    let src = testdata("module/pkg/sub");
    let pkg = Context::default()
        .import("example.com/mod", &src, ImportMode::NONE)
        .unwrap();
    assert_eq!(pkg.name, "mod");
    assert_eq!(pkg.go_files, vec!["mod.go"]);
}

#[test]
fn import_dir_sets_canonical_import_path_in_module() {
    let dir = testdata("module/pkg/sub");
    let pkg = Context::default().import_dir(&dir).unwrap();
    assert_eq!(pkg.name, "sub");
    assert_eq!(pkg.import_path, "example.com/mod/pkg/sub");
}

#[test]
fn is_local_import_paths() {
    use guff_build::is_local_import;
    assert!(is_local_import("."));
    assert!(is_local_import(".."));
    assert!(is_local_import("./foo"));
    assert!(is_local_import("../foo"));
    assert!(!is_local_import("example.com/foo"));
}
