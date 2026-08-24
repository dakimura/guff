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
    typecheck_with_deps_ignored(pkg_id, main_path, deps, &[])
}

/// Like [`typecheck_with_deps`], plus the package's build-constraint-excluded
/// files (`go list`'s `IgnoredGoFiles`). Analyzers that ask whether the package
/// has files they cannot see — `modernize`'s `atomictypes` is one — have no
/// other way to be reached from a fixture.
pub fn typecheck_with_deps_ignored(
    pkg_id: &str,
    main_path: &Path,
    deps: &[(&str, &Path)],
    ignored_files: &[&str],
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

    // Populate `imports` from the AST so `run_on_packages`' import gate
    // (`analyzer_applies_to_package` → `package_imports_prefix`) can see them.
    // The gate only inspects keys; stub values are enough. Without this, every
    // gated analyzer (testifylint, exptostd, sloglint, …) is silently skipped
    // and its "must flag" tests assert on an empty diagnostic list. Same fix as
    // X-2 made for the guff-govet harness; see docs/PERF_TASKS_V2.md §X-4.
    let mut imports = HashMap::new();
    for imp in &main_file.imports {
        let path = unquote_import_path(&imp.path.value);
        if !path.is_empty() {
            imports.insert(path, Arc::new(Package::default()));
        }
    }
    for (import_path, _) in deps {
        imports
            .entry((*import_path).to_string())
            .or_insert_with(|| Arc::new(Package::default()));
    }

    Arc::new(Package {
        id: pkg_id.into(),
        pkg_path: pkg_id.into(),
        name: main_file.name.name.clone().into(),
        dir: main_path.parent().unwrap_or(main_path).to_path_buf(),
        compiled_go_files: vec![main_path.to_path_buf()],
        go_files: vec![main_path.to_path_buf()],
        ignored_files: ignored_files.iter().map(PathBuf::from).collect(),
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
        imports,
        types_sizes: Some(default_sizes()),
        ..Package::default()
    })
}

/// Strip matching outer `"` / `` ` `` from an import path literal (AST keeps quotes).
fn unquote_import_path(lit: &str) -> String {
    let bytes = lit.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'`' && last == b'`') {
            return lit[1..lit.len() - 1].to_string();
        }
    }
    lit.to_string()
}

pub fn typecheck_pkg(pkg_id: &str, main_path: &Path) -> Arc<Package> {
    typecheck_with_deps(pkg_id, main_path, &[])
}

/// [`typecheck_fixture`] with the package's build-excluded file names attached.
pub fn typecheck_fixture_with_ignored_files(
    name: &str,
    pkg_id: &str,
    file: &str,
    ignored_files: &[&str],
) -> Arc<Package> {
    let dir = testdata(name);
    let stubs = collect_stubs(&dir);
    let stub_refs: Vec<(&str, &Path)> = stubs
        .iter()
        .map(|(p, path)| (p.as_str(), path.as_path()))
        .collect();
    typecheck_with_deps_ignored(pkg_id, &dir.join(file), &stub_refs, ignored_files)
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

pub fn run_analyzer(
    analyzer: &'static guff_analysis::Analyzer,
    pkg: &Arc<Package>,
) -> Vec<String> {
    run_analyzer_with_settings(analyzer, pkg, &RunnerOptions::default())
}

/// The diagnostics themselves, not just their messages.
///
/// A suggested fix's replacement text and span appear in no compat key — the
/// golden tier's is `path:line:col:linter:severity:text` — so a check whose
/// `--fix` writes the wrong bytes is invisible to every finding-set comparison.
/// Asserting on them here is the only unit-level net under `compat/fix/`.
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
