//! Integration tests for [`guff_build::Context::import_dir`].

use std::path::PathBuf;

use guff_build::{BuildError, Context};

fn testdata(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata")
        .join(path)
}

fn ctx(goos: &str) -> Context {
    Context {
        goos: goos.to_string(),
        goarch: "amd64".to_string(),
        ..Context::default()
    }
}

#[test]
fn import_dir_classifies_files_on_linux() {
    let dir = testdata("simple");
    let pkg = ctx("linux").import_dir(&dir).unwrap();
    assert_eq!(pkg.name, "foo");
    assert_eq!(pkg.go_files, vec!["bar_linux.go", "foo.go"]);
    assert_eq!(pkg.test_go_files, vec!["foo_test.go"]);
    assert!(pkg.ignored_go_files.is_empty());
}

#[test]
fn import_dir_ignores_linux_file_on_darwin() {
    let dir = testdata("simple");
    let pkg = ctx("darwin").import_dir(&dir).unwrap();
    assert_eq!(pkg.go_files, vec!["foo.go"]);
    assert_eq!(pkg.ignored_go_files, vec!["bar_linux.go"]);
}

#[test]
fn import_dir_no_go_when_only_tagged_out_files() {
    let base = std::env::temp_dir().join(format!("guff_build_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(
        base.join("bar_linux.go"),
        include_str!("testdata/simple/bar_linux.go"),
    )
    .unwrap();

    let err = ctx("darwin").import_dir(&base).unwrap_err();
    let _ = std::fs::remove_dir_all(&base);
    assert!(matches!(err, BuildError::NoGo(_)));
}
