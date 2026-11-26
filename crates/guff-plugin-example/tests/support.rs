use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_packages::{Error, ErrorKind, Package, TypecheckArtifacts};
use guff_runner::{run_on_packages, RunnerOptions};
use guff_types::{default_sizes, Checker, Config};

pub fn testdata(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata")
        .join(name)
}

pub fn typecheck_pkg(pkg_id: &str, main_path: &Path) -> Arc<Package> {
    let fset = FileSet::new();
    let src = fs::read(main_path).expect("read source");
    let name = main_path
        .file_name()
        .and_then(|s| s.to_str())
        .expect("file name");
    let file = parse_file(&fset, name, &src, Mode::NONE).expect("parse");
    let syntax = vec![file];

    let mut check = Checker::new(Config::default());
    check.check_files(syntax.clone());

    let ill_typed = !check.errors.is_empty();
    let errors: Vec<Error> = check
        .errors
        .iter()
        .map(|e| Error {
            pos: if e.pos == 0 {
                String::new()
            } else {
                e.pos.to_string()
            },
            msg: e.msg.clone(),
            kind: ErrorKind::Type,
        })
        .collect();

    let pkg_name = syntax[0].name.name.clone().into();
    let dir = main_path.parent().unwrap_or(main_path).to_path_buf();

    Arc::new(Package {
        id: pkg_id.into(),
        pkg_path: pkg_id.into(),
        name: pkg_name,
        dir,
        compiled_go_files: vec![main_path.to_path_buf()],
        go_files: vec![main_path.to_path_buf()],
        syntax,
        fset: Some(fset),
        types: Some(check.pkg),
        types_info: Some(std::sync::Arc::new(check.info.clone())),
        type_artifacts: Some(TypecheckArtifacts {
            type_pkg: check.pkg,
            types: check.types,
            objects: check.objects,
            scopes: check.scopes,
            packages: check.packages,
            info: std::sync::Arc::new(check.info),
        }),
        ill_typed,
        errors,
        imports: HashMap::new(),
        types_sizes: Some(default_sizes()),
        ..Package::default()
    })
}

pub fn run_analyzer(
    analyzer: &'static guff_analysis::Analyzer,
    pkg: &Arc<Package>,
) -> Vec<String> {
    let result = run_on_packages(
        &[analyzer],
        std::slice::from_ref(pkg),
        &RunnerOptions {
            sequential: true,
            ..RunnerOptions::default()
        },
    )
    .expect("run analyzer");
    for action in result.graph.all_actions() {
        if let Some(err) = action.error() {
            panic!("analyzer {} failed: {err}", action.string_id());
        }
    }
    result
        .diagnostics()
        .into_iter()
        .map(|(_, d)| d.message)
        .collect()
}
