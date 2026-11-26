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

pub fn collect_stubs(dir: &Path) -> Vec<(String, PathBuf)> {
    let stub_dir = dir.join("stub");
    let mut deps = Vec::new();
    if !stub_dir.exists() {
        return deps;
    }
    let mut stack = vec![stub_dir.clone()];
    while let Some(cur) = stack.pop() {
        for entry in fs::read_dir(&cur).expect("read stub dir") {
            let entry = entry.expect("stub entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("go") {
                let rel = path.strip_prefix(&stub_dir).expect("stub path");
                let import_path = rel
                    .parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or("")
                    .replace(std::path::MAIN_SEPARATOR, "/");
                deps.push((import_path, path));
            }
        }
    }
    deps
}

pub fn typecheck_with_deps(
    pkg_id: &str,
    main_path: &Path,
    deps: &[(&str, &Path)],
) -> Arc<Package> {
    let fset = FileSet::new();
    let main_src = fs::read(main_path).expect("read main source");
    let main_name = main_path
        .file_name()
        .and_then(|s| s.to_str())
        .expect("main file name");
    let main_file = parse_file(&fset, main_name, &main_src, Mode::NONE).expect("parse main");

    let mut check = Checker::new(Config::default());
    for (import_path, dep_path) in deps {
        let src = fs::read(dep_path).expect("read dependency source");
        let name = dep_path
            .file_name()
            .and_then(|s| s.to_str())
            .expect("dep file name");
        let file = parse_file(&fset, name, &src, Mode::NONE).expect("parse dependency");
        check.add_dependency_source(*import_path, vec![file]);
    }
    check.check_files(vec![main_file.clone()]);

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
            main_path.display(),
            errors
        );
    }

    Arc::new(Package {
        id: pkg_id.into(),
        pkg_path: pkg_id.into(),
        name: main_file.name.name.clone().into(),
        dir: main_path.parent().unwrap_or(main_path).to_path_buf(),
        compiled_go_files: vec![main_path.to_path_buf()],
        go_files: vec![main_path.to_path_buf()],
        syntax: vec![main_file],
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

pub fn typecheck_pkg(pkg_id: &str, main_path: &Path) -> Arc<Package> {
    typecheck_with_deps(pkg_id, main_path, &[])
}

pub fn typecheck_fixture(name: &str, pkg_id: &str, file: &str) -> Arc<Package> {
    let dir = testdata(name);
    let main = dir.join(file);
    let stubs = collect_stubs(&dir);
    let deps: Vec<(&str, &Path)> = stubs
        .iter()
        .map(|(p, path)| (p.as_str(), path.as_path()))
        .collect();
    typecheck_with_deps(pkg_id, &main, &deps)
}

/// Like [`run_analyzer`], but keyed by source line — for checks whose every
/// finding carries the same message, where a substring assertion measures
/// nothing.
#[allow(dead_code)]
pub fn run_analyzer_lines(
    analyzer: &'static guff_analysis::Analyzer,
    pkg: &Arc<Package>,
) -> Vec<i64> {
    let fset = pkg.fset.clone().expect("fixture has a FileSet");
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
    let mut out: Vec<i64> = result
        .diagnostics()
        .into_iter()
        .map(|(_, d)| fset.position(guff::position::Pos(d.pos as i64)).line)
        .collect();
    out.sort_unstable();
    out
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
