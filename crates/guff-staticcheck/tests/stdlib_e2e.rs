//! Stdlib end-to-end: `go list` → typecheck → staticcheck analyzer.

use std::sync::Arc;

use guff_packages::{go_available, load, Config, LoadMode};
use guff_runner::{run_on_packages, RunnerOptions};
use guff_staticcheck::sa1019;

#[test]
fn stdlib_errors_package_runs_sa1019() {
    if !go_available() {
        eprintln!("skipping: go not found on PATH");
        return;
    }

    let cfg = Config {
        mode: LoadMode::LOAD_SYNTAX,
        ..Config::default()
    };
    let pkgs = load(&cfg, &["errors".to_string()]).expect("go list errors");
    let pkg = pkgs.first().expect("errors package");
    assert_eq!(pkg.name, "errors");
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);
    assert!(pkg.types_info.is_some(), "expected types info");
    assert!(pkg.fset.is_some(), "expected fset from LOAD_SYNTAX");

    let arc = Arc::new(pkg.clone());
    assert!(arc.fset.is_some(), "fset lost on clone");
    let result = run_on_packages(
        &[sa1019::analyzer()],
        std::slice::from_ref(&arc),
        &RunnerOptions {
            sequential: true,
            ..RunnerOptions::default()
        },
    )
    .expect("run SA1019 on stdlib/errors");

    for action in result.graph.all_actions() {
        assert!(
            action.error().is_none(),
            "analyzer {} failed: {:?}",
            action.string_id(),
            action.error()
        );
    }
}
