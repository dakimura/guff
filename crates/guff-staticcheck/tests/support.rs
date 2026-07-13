//! Shared helpers for guff-staticcheck integration tests.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_packages::{Error, ErrorKind, Package, LoadMode, TypecheckArtifacts};
use guff_types::{Checker, Config, Sizes};

pub fn testdata(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata")
        .join(name)
}

/// Collect `(import_path, source_path)` pairs from `dir/stub/**.go`.
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

/// Type-check a main file with dependency sources registered under `import_path`.
pub fn typecheck_with_deps(
    pkg_id: &str,
    main_path: &Path,
    deps: &[(&str, &Path)],
) -> Arc<Package> {
    typecheck_with_deps_and_sizes(pkg_id, main_path, deps, None)
}

/// Like [`typecheck_with_deps`] but sets `Package::types_sizes` for platform-specific checks.
pub fn typecheck_with_deps_and_sizes(
    pkg_id: &str,
    main_path: &Path,
    deps: &[(&str, &Path)],
    sizes: Option<Sizes>,
) -> Arc<Package> {
    let fset = FileSet::new();
    let main_src = fs::read(main_path).expect("read main source");
    let main_name = main_path
        .file_name()
        .and_then(|s| s.to_str())
        .expect("main file name");
    let main_file =
        parse_file(&fset, main_name, &main_src, Mode::NONE).expect("parse main file");

    let mut check = Checker::new(Config::default());
    let mut dep_files: HashMap<String, guff::ast::File> = HashMap::new();
    for (import_path, dep_path) in deps {
        let src = fs::read(dep_path).expect("read dependency source");
        let name = dep_path
            .file_name()
            .and_then(|s| s.to_str())
            .expect("dep file name");
        let file = parse_file(&fset, name, &src, Mode::NONE).expect("parse dependency");
        dep_files.insert(import_path.to_string(), file.clone());
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

    let pkg_name = main_file.name.name.clone();
    let mut imports: HashMap<String, Arc<Package>> = HashMap::new();
    let artifacts_snapshot = TypecheckArtifacts {
        type_pkg: check.pkg,
        types: check.types.clone(),
        objects: check.objects.clone(),
        scopes: check.scopes.clone(),
        packages: check.packages.clone(),
        info: check.info.clone(),
    };
    for (import_path, dep_file) in &dep_files {
        let Some(type_pkg) = check.packages.find_by_path(import_path) else {
            continue;
        };
        imports.insert(
            import_path.clone(),
            Arc::new(Package {
                id: import_path.clone(),
                pkg_path: import_path.clone(),
                name: dep_file.name.name.clone().into(),
                dir: deps
                    .iter()
                    .find(|(p, _)| *p == import_path)
                    .map(|(_, path)| path.parent().unwrap_or(path).to_path_buf())
                    .unwrap_or_default(),
                compiled_go_files: vec![deps
                    .iter()
                    .find(|(p, _)| *p == import_path)
                    .map(|(_, path)| (*path).to_path_buf())
                    .unwrap_or_default()],
                go_files: vec![deps
                    .iter()
                    .find(|(p, _)| *p == import_path)
                    .map(|(_, path)| (*path).to_path_buf())
                    .unwrap_or_default()],
                syntax: vec![dep_file.clone()],
                fset: Some(fset.clone()),
                types: Some(type_pkg),
                types_info: Some(check.info.clone()),
                type_artifacts: Some(artifacts_snapshot.clone()),
                ill_typed,
                errors: errors.clone(),
                types_sizes: sizes,
                ..Package::default()
            }),
        );
    }

    Arc::new(Package {
        id: pkg_id.into(),
        pkg_path: pkg_id.into(),
        name: pkg_name.into(),
        dir: main_path.parent().unwrap_or(main_path).to_path_buf(),
        compiled_go_files: vec![main_path.to_path_buf()],
        go_files: vec![main_path.to_path_buf()],
        syntax: vec![main_file],
        fset: Some(fset),
        types: Some(check.pkg),
        types_info: Some(check.info.clone()),
        type_artifacts: Some(TypecheckArtifacts {
            type_pkg: check.pkg,
            types: check.types,
            objects: check.objects,
            scopes: check.scopes,
            packages: check.packages,
            info: check.info,
        }),
        ill_typed,
        errors,
        imports,
        types_sizes: sizes,
        ..Package::default()
    })
}

/// Type-check a single Go file with no imports (same helper as S1002 tests).
pub fn typecheck_file(dir: &Path, file: &str, id: &str) -> Arc<Package> {
    typecheck_with_deps(id, &dir.join(file), &[])
}

/// Assert the package type-checked cleanly.
pub fn assert_well_typed(pkg: &Package) {
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);
    assert!(pkg.types_info.is_some(), "missing types info");
}

/// Set the module Go version on a type-checked package (for version-gated checks).
pub fn with_go_version(mut pkg: Arc<Package>, version: &str) -> Arc<Package> {
    Arc::make_mut(&mut pkg).module = Some(guff_packages::Module {
        go_version: version.to_string(),
        ..guff_packages::Module::default()
    });
    pkg
}

/// Run analyzers and return diagnostic messages.
pub fn run_analyzer(
    analyzer: &'static guff_analysis::Analyzer,
    pkg: &Arc<Package>,
) -> Vec<String> {
    use guff_runner::{run_on_packages, RunnerOptions};

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

/// Silence unused import warning for LoadMode in future tests.
#[allow(dead_code)]
const _LOAD_SYNTAX: LoadMode = LoadMode::LOAD_SYNTAX;
