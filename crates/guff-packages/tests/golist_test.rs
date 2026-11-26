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

// `golist_populates_export_file_after_build` lives in its own binary
// (`golist_export_file.rs`): it has to set process-wide environment to pin a
// driver, which is not safe to do beside a second test in the same process.
