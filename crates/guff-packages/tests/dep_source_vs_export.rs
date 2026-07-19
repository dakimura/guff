//! Differential test: resolving a dependency's type information by
//! type-checking its **source** (the cold-path source seed) must agree with
//! resolving it from compiler **export data** (`.a`).
//!
//! Uses the `withdep` + `simple` fixtures. `main.go` does `const D = simple.X`,
//! and the `simple` dependency ships both `simple.go` (source) and `simple.a`
//! (export data), so this test needs **no `go` toolchain** — the packages are
//! built by hand and pointed at the on-disk fixtures.

use std::path::PathBuf;
use std::sync::Arc;

use guff_packages::{typecheck_roots, LoadMode, Package, TypecheckEnv};

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Build the (dep, root) package set. To isolate the mechanism under test, the
/// `simple` dependency is given **only** export data (`simple.a`) in export mode
/// and **only** source (`simple.go`) in source mode — so source mode genuinely
/// exercises the source importer rather than falling back to export data.
fn packages(from_source: bool) -> (Vec<Arc<Package>>, Vec<String>) {
    let withdep_dir = manifest().join("tests/testdata/typecheck/withdep");
    let simple_dir = manifest().join("../guff-exportdata/tests/testdata/export/simple");

    let simple = Package {
        id: "example.com/simple".into(),
        pkg_path: "example.com/simple".into(),
        dir: simple_dir.clone(),
        compiled_go_files: if from_source {
            vec![simple_dir.join("simple.go")]
        } else {
            Vec::new()
        },
        export_file: if from_source {
            Default::default()
        } else {
            simple_dir.join("simple.a")
        },
        ..Default::default()
    };
    let main = Package {
        id: "example.com/withdep".into(),
        pkg_path: "example.com/withdep".into(),
        dir: withdep_dir.clone(),
        compiled_go_files: vec![withdep_dir.join("main.go")],
        deps: vec!["example.com/simple".into()],
        ..Default::default()
    };

    (
        vec![Arc::new(simple), Arc::new(main)],
        vec!["example.com/withdep".into()],
    )
}

fn run(from_source: bool) -> Arc<Package> {
    let (all, targets) = packages(from_source);
    let env = TypecheckEnv {
        from_source,
        ..TypecheckEnv::default()
    };
    typecheck_roots(&all, &targets, LoadMode::LOAD_ALL_SYNTAX, &env)
        .into_iter()
        .next()
        .expect("one target package back")
}

/// Both paths must type-check `main.go` (which references `simple.X`) without
/// errors, and produce identical target-level diagnostics.
#[test]
fn source_seed_matches_export_for_withdep() {
    let export = run(false);
    let source = run(true);

    // If the dependency symbol `simple.X` failed to resolve, `main` would be
    // ill-typed ("undefined: simple.X"). So `!ill_typed` on the source path
    // already proves the dependency's exported const resolved *from source*.
    assert!(
        !export.ill_typed,
        "export path ill-typed: {:?}",
        export.errors
    );
    assert!(
        !source.ill_typed,
        "source path ill-typed: {:?}",
        source.errors
    );

    assert!(export.types_info.is_some(), "export path produced type info");
    assert!(source.types_info.is_some(), "source path produced type info");

    let diag = |p: &Package| -> Vec<(String, String)> {
        p.errors
            .iter()
            .map(|e| (e.pos.clone(), e.msg.clone()))
            .collect()
    };
    assert_eq!(
        diag(&export),
        diag(&source),
        "target diagnostics differ between export and source paths"
    );
}
