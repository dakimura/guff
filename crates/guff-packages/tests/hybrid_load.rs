//! End-to-end test of the hybrid source mode (`Config::dep_source`) through the
//! real `go list` driver: third-party dependencies are type-checked from source
//! (no `-export` build for them) while stdlib is resolved from export data built
//! by a second, stdlib-only `go list -export` call.
//!
//! The `hybrid` fixture's `main.go` uses both `fmt` (stdlib → export) and a
//! local `example.com/dep` (third-party → source), so a well-typed root proves
//! both resolution paths work together. Requires `go` on PATH.

use std::path::PathBuf;

use guff_packages::{go_available, load, Config, LoadMode};

#[test]
#[ignore = "requires go on PATH; run with cargo test -p guff-packages -- --ignored"]
fn hybrid_source_mode_is_well_typed() {
    if !go_available() {
        eprintln!("skipping: go not found on PATH");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/typecheck/hybrid");
    let cfg = Config {
        mode: LoadMode::LOAD_ALL_SYNTAX,
        dir,
        dep_source: true,
        ..Config::default()
    };

    let pkgs = load(&cfg, &["./...".to_string()]).expect("load hybrid module");
    let root = pkgs
        .iter()
        .find(|p| p.pkg_path == "example.com/hybrid")
        .expect("root package example.com/hybrid");

    assert!(
        !root.ill_typed,
        "hybrid root ill-typed (fmt+dep should both resolve): {:?}",
        root.errors
    );
    // In source mode the root itself is never given export data.
    assert!(
        root.export_file.as_os_str().is_empty(),
        "root should carry no export data in source mode, got {:?}",
        root.export_file
    );
}
