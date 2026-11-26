//! Regression: export-decoded named interfaces must preserve method `Pkg`.
//!
//! `testing.TB` seals itself with unexported `private()`. Go's ureader copies
//! `fn.Pkg()` when rewriting named-interface methods (#49906). Dropping pkg
//! made Implements fail for `*testing.T` → `testing.TB` (C-3d findings gap).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use guff::ast::File;
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_exportdata::ExportImporter;
use guff_types::{Checker, Config};

fn go_available() -> bool {
    Command::new("go")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn parse(src: &str) -> File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse")
}

fn build_sealed_archive(dir: &std::path::Path) -> PathBuf {
    fs::write(
        dir.join("go.mod"),
        "module example.com/sealed\n\ngo 1.22\n",
    )
    .unwrap();
    fs::write(
        dir.join("sealed.go"),
        r#"
package sealed

// I is sealed by unexported private(), like testing.TB.
type I interface {
	M()
	private()
}

type T struct{}

func (*T) M()       {}
func (*T) private() {}

func Accept(i I) {}
"#,
    )
    .unwrap();

    let out = Command::new("go")
        .args([
            "list",
            "-export",
            "-f",
            "{{.Export}}",
            "example.com/sealed",
        ])
        .current_dir(dir)
        .env("GO111MODULE", "on")
        .output()
        .expect("go list -export");
    assert!(
        out.status.success(),
        "go list -export failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let export = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(!export.is_empty(), "empty Export path");
    let export_path = PathBuf::from(&export);
    assert!(export_path.exists(), "missing export {export}");
    // Copy into the temp dir so the test does not depend on GOCACHE lifetime.
    let dest = dir.join("sealed.a");
    fs::copy(&export_path, &dest).expect("copy .a");
    dest
}

#[test]
fn export_sealed_iface_assignable_keeps_method_pkg() {
    if !go_available() {
        eprintln!("skipping: go not on PATH");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let archive = build_sealed_archive(dir.path());

    let fset = FileSet::new();
    let mut importer = ExportImporter::with_fset(fset);
    importer.set_path("example.com/sealed", archive);

    let mut check = Checker::new(Config::default());
    check.set_importer(Box::new(importer));

    let src = r#"
package main

import "example.com/sealed"

func f(t *sealed.T) {
	sealed.Accept(t)
}
"#;
    check.check_files(vec![parse(src)]);
    assert!(
        check.errors.is_empty(),
        "sealed iface assignability failed (method Pkg likely dropped in prepare_named_underlying): {:?}",
        check.errors
    );
}

#[test]
fn export_testing_tb_assignable_from_star_t() {
    if !go_available() {
        eprintln!("skipping: go not on PATH");
        return;
    }
    let out = Command::new("go")
        .args(["list", "-export", "-f", "{{.Export}}", "testing"])
        .output()
        .expect("go list testing");
    assert!(
        out.status.success(),
        "go list testing: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let export = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(!export.is_empty() && PathBuf::from(&export).exists());

    let fset = FileSet::new();
    let mut importer = ExportImporter::with_fset(fset);
    importer.set_path("testing", PathBuf::from(export));

    let mut check = Checker::new(Config::default());
    check.set_importer(Box::new(importer));

    let src = r#"
package main

import "testing"

func helper(tb testing.TB) {}

func f(t *testing.T) {
	helper(t)
}
"#;
    check.check_files(vec![parse(src)]);
    assert!(
        check.errors.is_empty(),
        "*testing.T should be assignable to testing.TB from export data: {:?}",
        check.errors
    );
}

#[test]
fn export_embed_fs_assignable_to_io_fs() {
    if !go_available() {
        eprintln!("skipping: go not on PATH");
        return;
    }
    // Map every package in embed's export closure so topo preload can resolve
    // cross-package named types (io/fs.File) to a single PackageId.
    let out = Command::new("go")
        .args([
            "list",
            "-export",
            "-deps",
            "-f",
            "{{.ImportPath}}\t{{.Export}}",
            "embed",
        ])
        .output()
        .expect("go list -export -deps embed");
    assert!(
        out.status.success(),
        "go list -export -deps embed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let fset = FileSet::new();
    let mut importer = ExportImporter::with_fset(fset);
    let mut paths: Vec<String> = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some((path, export)) = line.split_once('\t') else {
            continue;
        };
        if export.is_empty() || path == "unsafe" {
            continue;
        }
        let export_path = PathBuf::from(export);
        if !export_path.exists() {
            continue;
        }
        importer.set_path(path, export_path);
        paths.push(path.to_string());
    }
    assert!(
        paths.iter().any(|p| p == "embed") && paths.iter().any(|p| p == "io/fs"),
        "expected embed and io/fs in export closure, got {paths:?}"
    );

    let mut check = Checker::new(Config::default());
    check.set_importer(Box::new(importer));
    // Dependency-first order matches guff-packages preload_exports.
    for path in &paths {
        check.preload_import(path);
    }

    let src = r#"
package main

import (
	"embed"
	"io/fs"
)

func use(f fs.FS) {}

func f(e embed.FS) {
	use(e)
}
"#;
    check.check_files(vec![parse(src)]);
    assert!(
        check.errors.is_empty(),
        "embed.FS should be assignable to fs.FS from export data: {:?}",
        check.errors
    );
}
