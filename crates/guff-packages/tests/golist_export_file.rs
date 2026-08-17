//! `NEED_EXPORT_FILE` must come back with a path, on the driver that can serve it.
//!
//! Its own binary because it pins two drivers through the process environment,
//! and `set_var` beside another test's `var` read is a race.
//!
//! Two defaults moved under this assertion since it was written, and because it
//! carries `#[ignore]` and nothing on CI ran ignored tests, neither showed up:
//!
//!   * hybrid source mode (`dep_source`, default on) suppresses `-export`
//!     outright — golist.rs `uses_export_data` returns false before it ever
//!     looks at the mode, because skipping that dependency build is the point.
//!   * the native list driver (`GUFF_NATIVE_LIST`, default on) answers without
//!     shelling out to `go list` at all, and `native_or_golist` has no
//!     export-data arm: `list_config_from` does not bail on it, and
//!     `attach_hybrid_exports` returns early when `dep_source` is off. So a
//!     caller taking the documented `GUFF_DEP_SOURCE=0` escape hatch gets an
//!     empty `export_file` for every package with no fallback and no error.
//!
//! The second one is a live gap, not a stale expectation, and it is why this
//! test pins `GUFF_NATIVE_LIST=off` rather than asserting on the default: what
//! is verified here is the `go list -export` wiring. Delete the pin once the
//! native driver bails to `go list` when export data is requested.

use std::path::PathBuf;

use guff_packages::{go_available, load, Config, LoadMode};

#[test]
#[ignore = "requires go on PATH; run with cargo test -p guff-packages -- --ignored"]
fn golist_populates_export_file_after_build() {
    if !go_available() {
        eprintln!("skipping: go not found on PATH");
        return;
    }

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/golist");
    let status = std::process::Command::new("go")
        .args(["build", "-o", "/dev/null", "."])
        .current_dir(&dir)
        .status()
        .expect("spawn go build");
    assert!(status.success(), "go build in testdata failed");

    // Sole test in this binary, so nothing else is reading the environment.
    std::env::set_var("GUFF_NATIVE_LIST", "off");

    let cfg = Config {
        mode: LoadMode::LOAD_IMPORTS | LoadMode::NEED_EXPORT_FILE,
        dir: dir.clone(),
        // Ask for the export-data path explicitly; the hybrid default would
        // drop `-export` and make the assertion below vacuous.
        dep_source: false,
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
