use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guff::parser::{parse_file, Mode, PARSE_COMMENTS};
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
    let parse_mode = Mode::NONE | PARSE_COMMENTS;
    let main_file = parse_file(&fset, main_name, &main_src, parse_mode).expect("parse main");

    let mut check = Checker::new(Config::default());
    for (import_path, dep_path) in deps {
        let src = fs::read(dep_path).expect("read dependency source");
        let name = dep_path
            .file_name()
            .and_then(|s| s.to_str())
            .expect("dep file name");
        let file = parse_file(&fset, name, &src, parse_mode).expect("parse dependency");
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

pub fn typecheck_fixture(name: &str, pkg_id: &str, file: &str) -> Arc<Package> {
    let dir = testdata(name);
    let stubs = collect_stubs(&dir);
    let stub_refs: Vec<(&str, &Path)> = stubs
        .iter()
        .map(|(p, path)| (p.as_str(), path.as_path()))
        .collect();
    typecheck_with_deps(pkg_id, &dir.join(file), &stub_refs)
}

/// Type-check every `*.go` file in `tests/testdata/<name>/<subdir>/` as one package.
pub fn typecheck_fixture_dir(name: &str, subdir: &str, pkg_id: &str) -> Arc<Package> {
    let dir = testdata(name).join(subdir);
    let stubs = collect_stubs(&testdata(name));
    let mut go_files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("read fixture dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("go"))
        .collect();
    go_files.sort();
    assert!(!go_files.is_empty(), "no go files in {}", dir.display());

    let fset = FileSet::new();
    let parse_mode = Mode::NONE; // match production load (docs via rule reparse)
    let mut syntax = Vec::new();
    for path in &go_files {
        let src = fs::read(path).expect("read fixture");
        let name = path.file_name().and_then(|s| s.to_str()).expect("name");
        syntax.push(parse_file(&fset, name, &src, parse_mode).expect("parse"));
    }

    let mut check = Checker::new(Config::default());
    for (import_path, dep_path) in &stubs {
        let src = fs::read(dep_path).expect("read dependency source");
        let name = dep_path
            .file_name()
            .and_then(|s| s.to_str())
            .expect("dep file name");
        let file = parse_file(&fset, name, &src, parse_mode | PARSE_COMMENTS).expect("parse dep");
        check.add_dependency_source(import_path, vec![file]);
    }
    check.check_files(syntax.clone());

    let pkg_name = syntax[0].name.name.clone();
    Arc::new(Package {
        id: pkg_id.into(),
        pkg_path: pkg_id.into(),
        name: pkg_name.into(),
        dir,
        compiled_go_files: go_files.clone(),
        go_files,
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
        ill_typed: !check.errors.is_empty(),
        errors: Vec::new(),
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

/// Like [`run_analyzer`], but each entry is `"line:col: message"`.
///
/// Most revive assertions here only need the message, because the golden tier
/// (`compat/golden/run.sh`) is what compares columns against golangci-lint.
/// A rule whose column is only wrong on a *shape the fixtures do not contain*
/// stays invisible to both, though — `duplicated-imports` reported the import
/// path's column instead of the ImportSpec's for years, and could not be caught
/// until an aliased duplicate existed to tell the two apart.
pub fn run_analyzer_at(
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
    let fset = pkg.fset.as_ref().expect("fixture package has a FileSet");
    let mut out: Vec<String> = result
        .diagnostics()
        .into_iter()
        .map(|(_, d)| {
            let pos = fset.position(guff::position::Pos(i64::from(d.pos)));
            let col = d.column.map_or(pos.column, i64::from);
            format!("{}:{}: {}", pos.line, col, d.message)
        })
        .collect();
    out.sort();
    out
}

/// The diagnostics themselves, not just their messages.
///
/// A `ReplacementLine` appears in no compat key — the golden tier's is
/// `path:line:col:linter:severity:text` — so a rule can report perfectly and
/// rewrite nothing, or rewrite the wrong span, with every finding gate green.
pub fn run_analyzer_diagnostics(
    analyzer: &'static guff_analysis::Analyzer,
    pkg: &Arc<Package>,
) -> Vec<guff_analysis::Diagnostic> {
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
    result.diagnostics().into_iter().map(|(_, d)| d).collect()
}
