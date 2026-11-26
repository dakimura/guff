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
    typecheck_pkg_files(pkg_id, &[main_path.to_path_buf()])
}

/// Typecheck a package assembled from multiple Go source files (same package).
pub fn typecheck_pkg_files(pkg_id: &str, paths: &[PathBuf]) -> Arc<Package> {
    assert!(!paths.is_empty(), "need at least one Go file");
    let fset = FileSet::new();
    let mut syntax = Vec::new();
    for path in paths {
        let src = fs::read(path).expect("read source");
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .expect("file name");
        let file = parse_file(&fset, name, &src, Mode::NONE).expect("parse");
        syntax.push(file);
    }

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

    if ill_typed {
        eprintln!(
            "warning: typecheck errors in {}: {:?}",
            paths[0].display(),
            errors
        );
    }

    let pkg_name = syntax[0].name.name.clone().into();
    let dir = paths[0].parent().unwrap_or(&paths[0]).to_path_buf();

    Arc::new(Package {
        id: pkg_id.into(),
        pkg_path: pkg_id.into(),
        name: pkg_name,
        dir,
        compiled_go_files: paths.to_vec(),
        go_files: paths.to_vec(),
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

pub fn typecheck_fixture(name: &str, pkg_id: &str, file: &str) -> Arc<Package> {
    let dir = testdata(name);
    typecheck_pkg(pkg_id, &dir.join(file))
}

pub fn typecheck_fixture_dir(name: &str, pkg_id: &str) -> Arc<Package> {
    let dir = testdata(name);
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("read fixture dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("go"))
        .collect();
    paths.sort();
    typecheck_pkg_files(pkg_id, &paths)
}

pub fn run_analyzer(
    analyzer: &'static guff_analysis::Analyzer,
    pkg: &Arc<Package>,
) -> Vec<String> {
    run_analyzer_with_settings(analyzer, pkg, &RunnerOptions::default())
}

/// Diagnostics as `line:column  message`, for the checks whose bug is a
/// position rather than a message.
pub fn run_analyzer_positions(
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
    let fset = pkg.fset.as_ref().expect("fset");
    result
        .diagnostics()
        .into_iter()
        .map(|(_, d)| {
            let p = fset.position(guff::Pos(d.pos as i64));
            format!("{}:{}  {}", p.line, p.column, d.message)
        })
        .collect()
}

pub fn run_analyzer_with_settings(
    analyzer: &'static guff_analysis::Analyzer,
    pkg: &Arc<Package>,
    options: &RunnerOptions,
) -> Vec<String> {
    let result = run_on_packages(
        &[analyzer],
        std::slice::from_ref(pkg),
        &RunnerOptions {
            sequential: true,
            ..options.clone()
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
