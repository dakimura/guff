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
    if !pkg_id.is_empty() {
        check.packages.get_mut(check.pkg).set_path(pkg_id.to_string());
    }
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

    if ill_typed {
        eprintln!(
            "warning: typecheck errors in {}: {:?}",
            main_path.display(),
            errors
        );
    }

    let mut imports: HashMap<String, Arc<Package>> = HashMap::new();
    let artifacts_snapshot = TypecheckArtifacts {
        type_pkg: check.pkg,
        types: check.types.clone(),
        objects: check.objects.clone(),
        scopes: check.scopes.clone(),
        packages: check.packages.clone(),
        info: std::sync::Arc::new(check.info.clone()),
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
                dir: PathBuf::new(),
                compiled_go_files: vec![],
                go_files: vec![],
                syntax: vec![dep_file.clone()],
                fset: Some(fset.clone()),
                types: Some(type_pkg),
                types_info: Some(std::sync::Arc::new(check.info.clone())),
                type_artifacts: Some(artifacts_snapshot.clone()),
                types_sizes: Some(default_sizes()),
                ..Package::default()
            }),
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
        imports,
        types_sizes: Some(default_sizes()),
        ..Package::default()
    })
}

pub fn typecheck_pkg(pkg_id: &str, main_path: &Path) -> Arc<Package> {
    let dir = main_path.parent().unwrap_or(main_path);
    let stubs = collect_stubs(dir);
    let deps: Vec<(&str, &Path)> = stubs
        .iter()
        .map(|(p, path)| (p.as_str(), path.as_path()))
        .collect();
    typecheck_with_deps(pkg_id, main_path, &deps)
}

pub fn run_analyzer(
    analyzer: &'static guff_analysis::Analyzer,
    pkg: &Arc<Package>,
) -> Vec<String> {
    run_analyzer_with_settings(analyzer, pkg, &RunnerOptions {
        sequential: true,
        ..RunnerOptions::default()
    })
}

pub fn run_analyzer_with_settings(
    analyzer: &'static guff_analysis::Analyzer,
    pkg: &Arc<Package>,
    opts: &RunnerOptions,
) -> Vec<String> {
    let mut opts = opts.clone();
    opts.sequential = true;
    let result = run_on_packages(&[analyzer], std::slice::from_ref(pkg), &opts).expect("run analyzer");
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
