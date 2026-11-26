//! SSA build from a type-checked `guff_packages::Package` (Phase 4, P4-d).

use std::path::PathBuf;

use guff_packages::{typecheck_package, LoadMode, Package, TypecheckEnv};
use guff_ssa::mode::BuilderMode;
use guff_ssa::ssautil::{build_package_from_loaded, build_package_from_source};
use guff_types::default_sizes;

const HELLO: &str = "package main\n\nfunc main() {\n\treturn\n}\n";

fn testdata_valid() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../guff-packages/tests/testdata/typecheck/valid")
}

#[test]
fn build_package_from_loaded_matches_from_source() {
    let dir = testdata_valid();
    let mut loaded = Package {
        id: "example.com/valid".into(),
        pkg_path: "example.com/valid".into(),
        dir: dir.clone(),
        compiled_go_files: vec![dir.join("main.go")],
        ..Package::default()
    };

    let fset = guff::position::FileSet::new();
    typecheck_package(
        &mut loaded,
        &fset,
        &std::collections::HashMap::default(),
        &std::collections::HashMap::default(),
        default_sizes(),
        &TypecheckEnv::default(),
        LoadMode::LOAD_SYNTAX,
    );
    assert!(!loaded.ill_typed, "{:?}", loaded.errors);

    let from_loaded = build_package_from_loaded(
        &mut loaded,
        BuilderMode::SANITY_CHECK_FUNCTIONS,
    )
    .expect("build from loaded package");

    let fset2 = guff::position::FileSet::new();
    let file = guff::parser::parse_file(&fset2, "main.go", HELLO.as_bytes(), guff::parser::Mode::NONE)
        .expect("parse");
    let from_source = build_package_from_source(
        fset2,
        guff_types::Config::default(),
        vec![file],
        BuilderMode::SANITY_CHECK_FUNCTIONS,
    )
    .expect("build from source");

    assert_eq!(
        from_loaded.prog.package_arena.get(from_loaded.type_pkg).name(),
        from_source.prog.package_arena.get(from_source.type_pkg).name(),
    );
    assert!(
        from_loaded.prog.packages.get(from_loaded.pkg).func("main").is_some(),
        "loaded path should build main"
    );
    assert!(
        from_source.prog.packages.get(from_source.pkg).func("main").is_some(),
        "source path should build main"
    );
}
