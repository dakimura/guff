//! `NEED_EXPORT_FILE` must come back with a path, on whichever driver is chosen.
//!
//! Two defaults moved under this assertion after it was written, and because it
//! carries `#[ignore]` and nothing on CI ran ignored tests, neither showed up:
//!
//!   * hybrid source mode (`dep_source`, default on) suppresses `-export`
//!     outright — golist.rs `uses_export_data` returns false before it ever
//!     looks at the mode, because skipping that dependency build is the point.
//!     So the export-data path has to be asked for explicitly below; a config
//!     that only sets `NEED_EXPORT_FILE` makes the assertion vacuous.
//!   * the native list driver (`GUFF_NATIVE_LIST`, default on) answers without
//!     shelling out to `go list` at all, and had no export-data arm: it now
//!     bails with `BailReason::ExportData` so `native_or_golist` falls back.
//!
//! Which is why this test does *not* pin a driver: run under the defaults, it
//! is the regression test for that fallback. Pinning `GUFF_NATIVE_LIST=off`
//! would still pass with the bail removed again.

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

    let cfg = Config {
        mode: LoadMode::LOAD_IMPORTS | LoadMode::NEED_EXPORT_FILE,
        dir: dir.clone(),
        // The export-data path, which is what `GUFF_DEP_SOURCE=0` selects.
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
