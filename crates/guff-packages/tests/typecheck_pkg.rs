//! Integration-style tests for package loading + type-checking (Phase 4).

use std::path::PathBuf;

use guff::ast::Decl;
use guff_packages::{load, typecheck_package, Config, LoadMode, TypecheckEnv};

fn testdata(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/typecheck")
        .join(name)
}

fn typecheck_fixture(dir: &PathBuf, go_file: &str, id: &str) -> guff_packages::Package {
    let mut pkg = guff_packages::Package {
        id: id.into(),
        pkg_path: id.into(),
        dir: dir.clone(),
        compiled_go_files: vec![dir.join(go_file)],
        ..Default::default()
    };
    let fset = guff::position::FileSet::new();
    typecheck_package(
        &mut pkg,
        &fset,
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        guff_types::default_sizes(),
        &TypecheckEnv::default(),
        LoadMode::LOAD_SYNTAX,
    );
    pkg
}

#[test]
fn typecheck_valid_fixture() {
    let dir = testdata("valid");
    let pkg = typecheck_fixture(&dir, "main.go", "example.com/valid");
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);
    let info = pkg.types_info.as_deref().expect("types info");
    let file = pkg.syntax.first().expect("syntax");
    let main_id = file
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::FuncDecl(fd) if fd.name.name == "main" => Some(fd.name.id),
            _ => None,
        })
        .expect("main");
    assert!(info.defs.contains_key(&main_id));
}

#[test]
fn typecheck_invalid_fixture_is_ill_typed() {
    let dir = testdata("invalid");
    let pkg = typecheck_fixture(&dir, "bad.go", "example.com/invalid");
    assert!(pkg.ill_typed);
}

#[test]
#[ignore = "requires go toolchain"]
fn load_with_types_from_go_list() {
    if !guff_packages::go_available() {
        return;
    }
    let dir = testdata("valid");
    let cfg = Config {
        dir: dir.clone(),
        mode: LoadMode::LOAD_SYNTAX,
        ..Config::default()
    };
    let pkgs = load(&cfg, &[".".to_string()]).expect("load");
    assert_eq!(pkgs.len(), 1);
    assert!(!pkgs[0].ill_typed, "{:?}", pkgs[0].errors);
    assert!(pkgs[0].types.is_some());
    assert!(pkgs[0].types_info.is_some());
    assert!(!pkgs[0].syntax.is_empty());
}
