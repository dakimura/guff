//! R12: `--fix` applies suggested fixes to source files.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_lint::{analyzers_for_linter, apply_fixes, LintResult};
use guff_packages::{Error, ErrorKind, Package, TypecheckArtifacts};
use guff_runner::{run_on_packages, RunnerOptions};
use guff_staticcheck::sa1004;
use guff_types::{Checker, Config as TypeConfig};
use tempfile::TempDir;

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/fix")
        .join(name)
}

fn copy_fixture_named(dir: &Path, name: &str) -> std::io::Result<()> {
    for entry in fs::read_dir(fixture_dir(name))? {
        let entry = entry?;
        let path = entry.path();
        let dest = dir.join(entry.file_name());
        if path.is_dir() {
            copy_dir_all(&path, &dest)?;
        } else {
            fs::copy(&path, dest)?;
        }
    }
    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

fn collect_stubs(dir: &Path) -> Vec<(String, PathBuf)> {
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

fn typecheck_sa1004_fixture(dir: &Path) -> Arc<Package> {
    let main_path = dir.join("bad.go");
    let fset = FileSet::new();
    let main_src = fs::read(&main_path).expect("read bad.go");
    let main_file = parse_file(
        &fset,
        main_path.to_str().expect("utf8 path"),
        &main_src,
        Mode::NONE,
    )
    .expect("parse bad.go");

    let mut check = Checker::new(TypeConfig::default());
    let mut dep_files: HashMap<String, guff::ast::File> = HashMap::new();
    let stubs = collect_stubs(dir);
    for (import_path, dep_path) in &stubs {
        let src = fs::read(dep_path).expect("read stub");
        let file = parse_file(
            &fset,
            dep_path.to_str().expect("utf8 path"),
            &src,
            Mode::NONE,
        )
        .expect("parse stub");
        dep_files.insert(import_path.clone(), file.clone());
        check.add_dependency_source(import_path, vec![file]);
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
                dir: stubs
                    .iter()
                    .find(|(p, _)| p == import_path)
                    .map(|(_, path)| path.parent().unwrap_or(path).to_path_buf())
                    .unwrap_or_default(),
                compiled_go_files: vec![stubs
                    .iter()
                    .find(|(p, _)| p == import_path)
                    .map(|(_, path)| path.to_path_buf())
                    .unwrap_or_default()],
                syntax: vec![dep_file.clone()],
                fset: Some(fset.clone()),
                types: Some(type_pkg),
                types_info: Some(std::sync::Arc::new(check.info.clone())),
                type_artifacts: Some(artifacts_snapshot.clone()),
                ill_typed,
                errors: errors.clone(),
                ..Package::default()
            }),
        );
    }

    Arc::new(Package {
        id: "example.com/staticcheck/sa1004".into(),
        pkg_path: "example.com/staticcheck/sa1004".into(),
        name: pkg_name.into(),
        dir: dir.to_path_buf(),
        compiled_go_files: vec![main_path],
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
        ..Package::default()
    })
}

#[test]
fn sa1004_offers_two_conflicting_fixes_so_nothing_is_written() {
    // This test used to assert the opposite — that `--fix` rewrites the literal
    // to `1 * time.Nanosecond`. That was guff emitting one suggested fix where
    // upstream emits two, "Explicitly use nanoseconds" and "Use seconds", over
    // the same span. golangci gathers every suggested fix's edits into one list
    // per linter, so the two overlap and the conflict pass drops all of
    // staticcheck's edits for the file. golangci-lint 2.12.2 leaves this file
    // untouched, and now so does guff.
    let dir = TempDir::new().unwrap();
    copy_fixture_named(dir.path(), "sa1004").unwrap();

    let pkg = typecheck_sa1004_fixture(dir.path());
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);

    let run = run_on_packages(
        &[sa1004::analyzer()],
        std::slice::from_ref(&pkg),
        &RunnerOptions {
            sequential: true,
            ..RunnerOptions::default()
        },
    )
    .expect("run SA1004");

    let result = LintResult {
        packages: vec![pkg],
        run,
        filter: guff_lint::IssueFilter::default(),
        cached_issues: Vec::new(),
        path_mode: guff_lint::PathMode::Rel,
        path_prefix: None,
    };

    let fset = result
        .packages
        .first()
        .and_then(|p| p.fset.as_ref())
        .expect("fset");
    let issues = result.issues();
    assert_eq!(issues.len(), 2, "{issues:?}");

    let bad = dir.path().join("bad.go");
    let before = fs::read_to_string(&bad).unwrap();
    let (remaining, n) = apply_fixes(fset, &issues, None).unwrap();
    assert_eq!(n, 0, "both fixes lose the conflict");
    assert_eq!(
        remaining.len(),
        2,
        "guff keeps reporting what it did not fix (COMPAT-HARDENING 続き 37)"
    );

    assert_eq!(
        fs::read_to_string(&bad).unwrap(),
        before,
        "the file must be untouched, as upstream leaves it"
    );
}

#[test]
fn cli_fix_flag_applies_and_clears_output() {
    // S1002, not SA1004: SA1004's two competing fixes conflict and nothing is
    // written, which is right but makes it useless for exercising the applied
    // path. S1002 offers one fix per finding.
    let dir = TempDir::new().unwrap();
    copy_fixture_named(dir.path(), "s1002").unwrap();

    let guff = env!("CARGO_BIN_EXE_guff");
    let out = Command::new(guff)
        .current_dir(dir.path())
        .args([
            "run",
            "--no-cache",
            "--enable",
            "staticcheck",
            "--disable",
            "errcheck",
            "--disable",
            "govet",
            "--disable",
            "ineffassign",
            "--disable",
            "unused",
            "--fix",
            "--issues-exit-code",
            "1",
            ".",
        ])
        .output()
        .expect("guff run --fix");

    assert!(
        out.status.success(),
        "stderr={}\nstdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout),
    );
    assert!(
        out.stdout.is_empty(),
        "fixed issues should not appear in stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("fixed 2 issue"),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let content = fs::read_to_string(dir.path().join("bad.go")).unwrap();
    assert!(content.contains("if b {"), "{content}");
    assert!(content.contains("if b {\n\t\t_ = b\n\t}\n\tif b {"), "{content}");
}

#[test]
fn staticcheck_sa1004_is_registered_for_fix_workflow() {
    let analyzers = analyzers_for_linter("staticcheck").expect("staticcheck");
    assert!(analyzers.iter().any(|a| a.name == "SA1004"));
    assert!(sa1004::analyzer().requires.iter().any(|a| a.name == "inspect"));
}
