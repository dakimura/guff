//! `build_package_for_analysis` preserves the loaded package.

use std::path::PathBuf;

use guff_packages::{typecheck_package, LoadMode, Package, TypecheckEnv};
use guff_ssa::mode::BuilderMode;
use guff_ssa::ssautil::build_package_for_analysis;
use guff_types::default_sizes;

#[test]
fn build_package_for_analysis_leaves_package_intact() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../guff-packages/tests/testdata/typecheck/valid");
    let mut pkg = Package {
        id: "example.com/valid".into(),
        pkg_path: "example.com/valid".into(),
        dir: dir.clone(),
        compiled_go_files: vec![dir.join("main.go")],
        ..Package::default()
    };
    let fset = guff::position::FileSet::new();
    typecheck_package(
        &mut pkg,
        &fset,
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        default_sizes(),
        &TypecheckEnv::default(),
        LoadMode::LOAD_SYNTAX,
    );
    assert!(!pkg.ill_typed);
    let artifacts = pkg.type_artifacts.as_ref().expect("artifacts").snapshot();
    let fset_arc = pkg.fset.clone().expect("fset");
    let files = pkg.syntax.clone();

    let built = build_package_for_analysis(
        artifacts,
        &files,
        fset_arc,
        BuilderMode::SANITY_CHECK_FUNCTIONS,
    )
    .expect("build");

    assert!(pkg.type_artifacts.is_some(), "package artifacts remain");
    assert!(!pkg.syntax.is_empty(), "syntax remains");
    assert!(
        built
            .prog
            .functions
            .iter()
            .any(|(_, f)| f.name == "main"),
        "SSA main exists"
    );
}
