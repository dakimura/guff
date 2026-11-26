//! Multi-root hybrid seed: when a dependency is also a pattern root with tests,
//! `filter_duplicate_packages` keeps only `P [P.test]`. The seed dep graph must
//! still resolve `P`'s imports by import path so embedded field types stay
//! valid (cli `api.HTTPError` / govet errorsas).

use std::path::PathBuf;

use guff_packages::{go_available, load, Config, LoadMode};

#[test]
#[ignore = "requires go on PATH; run with cargo test -p guff-packages -- --ignored"]
fn multiroot_test_variant_keeps_embedded_error_well_typed() {
    if !go_available() {
        eprintln!("skipping: go not found on PATH");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/typecheck/embed_multiroot");
    let cfg = Config {
        mode: LoadMode::LOAD_ALL_SYNTAX,
        dir,
        dep_source: true,
        tests: true,
        ..Config::default()
    };

    // Both app and lib are roots — lib survives only as lib [lib.test].
    let pkgs = load(
        &cfg,
        &["./app/...".to_string(), "./lib/...".to_string()],
    )
    .expect("load embed_multiroot");

    let app = pkgs
        .iter()
        .find(|p| p.pkg_path == "example.com/embedroot/app")
        .expect("app package");
    assert!(
        !app.ill_typed,
        "app ill-typed under multi-root (embedded *ext.Base should resolve): {:?}",
        app.errors
    );

    let lib = pkgs
        .iter()
        .find(|p| p.pkg_path == "example.com/embedroot/lib")
        .expect("lib package");
    assert!(
        !lib.ill_typed,
        "lib ill-typed (must typecheck after ext): {:?}",
        lib.errors
    );
    assert!(
        lib.id.contains(".test]"),
        "expected test-variant survivor id, got {}",
        lib.id
    );
}
