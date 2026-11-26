//! `time.UTC` must resolve to `*time.Location` when type-checking from source
//! seed (hybrid `dep_source`). Regression for init_var clobbering an explicit
//! var type when the initializer forward-refs a later package var (`utcLoc`).

use std::path::PathBuf;

use guff_packages::{go_available, load, Config, LoadMode};

#[test]
fn time_utc_well_typed_from_source_seed() {
    if !go_available() {
        return;
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/typecheck/time_utc");
    // Ensure stdlib is type-checked from source (the failing path), not .a.
    std::env::set_var("GUFF_STDLIB_SOURCE", "1");
    let cfg = Config {
        mode: LoadMode::LOAD_ALL_SYNTAX,
        dir,
        dep_source: true,
        ..Config::default()
    };
    let pkgs = load(&cfg, &[".".to_string()]).expect("load");
    let root = pkgs
        .iter()
        .find(|p| p.pkg_path == "example.com/time_utc")
        .expect("root package");
    assert!(
        !root.ill_typed,
        "time.UTC should typecheck from source seed: {:?}",
        root.errors
    );
}
