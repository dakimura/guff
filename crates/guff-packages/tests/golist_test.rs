//! Integration test for the `go list` driver.

use std::path::PathBuf;

use guff_packages::{go_available, load, Config, LoadMode};

fn testdata_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/golist")
}

#[test]
#[ignore = "requires go on PATH; run with cargo test -p guff-packages -- --ignored"]
fn golist_loads_mini_module() {
    if !go_available() {
        eprintln!("skipping: go not found on PATH");
        return;
    }

    let dir = testdata_dir();
    let cfg = Config {
        mode: LoadMode::LOAD_IMPORTS,
        dir: dir.clone(),
        ..Config::default()
    };

    let pkgs = load(&cfg, &[".".to_string()]).expect("load packages");
    assert!(!pkgs.is_empty(), "expected at least one package");

    let main_pkg = pkgs
        .iter()
        .find(|p| p.name == "main")
        .expect("main package");
    assert_eq!(main_pkg.pkg_path, "example.com/golist");
    assert!(!main_pkg.go_files.is_empty());
    assert!(
        main_pkg
            .go_files
            .iter()
            .any(|f| f.file_name().is_some_and(|n| n == "main.go"))
    );
    assert!(main_pkg.imports.contains_key("fmt"));
}

#[test]
#[ignore = "requires go on PATH; run with cargo test -p guff-packages -- --ignored"]
fn golist_populates_export_file_after_build() {
    if !go_available() {
        eprintln!("skipping: go not found on PATH");
        return;
    }

    let dir = testdata_dir();
    let status = std::process::Command::new("go")
        .args(["build", "-o", "/dev/null", "."])
        .current_dir(&dir)
        .status()
        .expect("spawn go build");
    assert!(status.success(), "go build in testdata failed");

    let cfg = Config {
        mode: LoadMode::LOAD_IMPORTS | LoadMode::NEED_EXPORT_FILE,
        dir: dir.clone(),
        ..Config::default()
    };

    let pkgs = load(&cfg, &[".".to_string()]).expect("load packages");
    let main_pkg = pkgs
        .iter()
        .find(|p| p.name == "main")
        .expect("main package");
    assert!(
        !main_pkg.export_file.as_os_str().is_empty(),
        "export_file should be non-empty after go build"
    );
}
