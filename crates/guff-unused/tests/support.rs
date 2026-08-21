use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_packages::{Error, ErrorKind, Package, TypecheckArtifacts};
use guff_runner::{run_on_packages, RunnerOptions};
use guff_types::{Checker, Config, default_sizes};

pub fn testdata(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata")
        .join(name)
}

pub fn typecheck_pkg(pkg_id: &str, main_path: &Path) -> Arc<Package> {
    typecheck_pkg_files(pkg_id, std::slice::from_ref(&main_path))
}

/// Same, for a package spread over more than one file. `unused` has rules that
/// only exist across files — a `//lint:file-ignore` in one file reaching the
/// methods of a type declared there but written in another — so a one-file
/// harness cannot express them.
pub fn typecheck_pkg_files(pkg_id: &str, paths: &[&Path]) -> Arc<Package> {
    let fset = FileSet::new();
    let main_path = paths.first().copied().expect("at least one source");
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let src = fs::read(path).expect("read source");
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .expect("file name");
        files.push(parse_file(&fset, name, &src, Mode::NONE).expect("parse source"));
    }
    let main_file = files[0].clone();

    let mut check = Checker::new(Config::default());
    check.check_files(files.clone());

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

    Arc::new(Package {
        id: pkg_id.into(),
        pkg_path: pkg_id.into(),
        name: main_file.name.name.clone().into(),
        dir: main_path.parent().unwrap_or(main_path).to_path_buf(),
        compiled_go_files: paths.iter().map(|p| p.to_path_buf()).collect(),
        go_files: paths.iter().map(|p| p.to_path_buf()).collect(),
        syntax: files,
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
