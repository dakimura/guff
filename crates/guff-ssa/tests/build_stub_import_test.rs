//! SSA build smoke for stub-import callcheck fixtures.

use std::fs;
use std::path::PathBuf;

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::mode::BuilderMode;
use guff_ssa::ssautil::build_package_for_analysis;
use guff_types::{Checker, Config};

fn build_stub_fixture(dir: &str, dep: (&str, &str)) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../guff-staticcheck/tests/testdata")
        .join(dir);
    let dep_stub = dir.join(dep.1);
    let main_src = fs::read_to_string(dir.join("bad.go")).expect("read bad.go");
    let fset = FileSet::new();
    let main_file =
        parse_file(&fset, "bad.go", main_src.as_bytes(), Mode::NONE).expect("parse main");

    let mut check = Checker::new(Config::default());
    let dep_src = fs::read_to_string(&dep_stub).expect("read stub");
    let dep_file =
        parse_file(&fset, "dep.go", dep_src.as_bytes(), Mode::NONE).expect("parse stub");
    check.add_dependency_source(dep.0, vec![dep_file]);
    check.check_files(vec![main_file.clone()]);

    assert!(check.errors.is_empty(), "{:?}", check.errors);

    let built = build_package_for_analysis(
        guff_packages::TypecheckArtifacts {
            type_pkg: check.pkg,
            types: check.types,
            objects: check.objects,
            scopes: check.scopes,
            packages: check.packages,
            info: std::sync::Arc::new(check.info),
        },
        std::slice::from_ref(&main_file),
        fset,
        BuilderMode::SANITY_CHECK_FUNCTIONS,
    )
    .expect("ssa build");

    let dep_type_pkg = built
        .prog
        .package_map
        .keys()
        .copied()
        .find(|id| built.prog.package_arena.get(*id).path() == dep.0)
        .expect("dependency import");
    let dep_ssa = built.prog.package_map[&dep_type_pkg];
    assert!(
        !built.prog.packages.get(dep_ssa).members.is_empty(),
        "no members for {}: {:?}",
        dep.0,
        built
            .prog
            .packages
            .get(dep_ssa)
            .members
            .keys()
            .collect::<Vec<_>>()
    );
}

#[test]
fn build_sa1024_stub_import_resolves_trimleft() {
    build_stub_fixture("sa1024", ("strings", "stub/strings/strings.go"));
}

#[test]
fn build_sa1002_stub_import_resolves_parse() {
    build_stub_fixture("sa1002", ("time", "stub/time/parse.go"));
}
