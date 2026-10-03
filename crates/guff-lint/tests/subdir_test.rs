//! `guff run ./...` from a subdirectory of the module, with the config at the
//! module root — the shape of a monorepo CI job that `cd`s into
//! `backend/<service>` and runs the linter there.
//!
//! Two things go wrong if either half is off, and both were:
//!
//! - `./...` is the working directory's packages, as with any `./` pattern.
//!   guff walked the whole module, so a service job linted every other
//!   service and the shared libraries too.
//! - The printed path is golangci's `RelativePath`: relative to the
//!   `run.relative-path-mode` directory, which by default is the config file's
//!   — not the working directory. Upstream prints `svc/app/app.go` here; guff
//!   printed `app/app.go`, so every line differed from golangci-lint's output.
//!
//! Every expectation below is what golangci-lint 2.12.2 printed for the same
//! tree.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

/// `lib` (imported by `svc/app`), `other` (imported by nothing) and
/// `svc/app`, each with one unchecked `os.Remove`.
fn module(name: &str, mode: Option<&str>) -> PathBuf {
    let root = std::env::temp_dir().join(format!("guff-subdir-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    write(&root, "go.mod", "module example.com/sub\n\ngo 1.24\n");
    let run = mode.map_or(String::new(), |m| format!("run:\n  relative-path-mode: {m}\n"));
    write(
        &root,
        ".golangci.yml",
        &format!("version: \"2\"\n{run}linters:\n  default: none\n  enable: [errcheck]\n"),
    );
    write(&root, "lib/lib.go", "package lib\n\nimport \"os\"\n\nfunc F() { os.Remove(\"x\") }\n");
    write(&root, "other/other.go", "package other\n\nimport \"os\"\n\nfunc G() { os.Remove(\"y\") }\n");
    write(
        &root,
        "svc/app/app.go",
        "package app\n\nimport (\n\t\"os\"\n\n\t\"example.com/sub/lib\"\n)\n\nfunc H() { lib.F(); os.Remove(\"z\") }\n",
    );
    root
}

/// The file part of each `file:line:col:` diagnostic, sorted.
fn files(root: &Path, cwd: &str, pattern: &str) -> Vec<String> {
    let cache = root.join(".cache");
    let out = Command::new(env!("CARGO_BIN_EXE_guff"))
        .args(["run", "--no-cache", "--issues-exit-code", "0", pattern])
        .env("GUFF_CACHE", &cache)
        .current_dir(root.join(cwd))
        .output()
        .expect("spawn guff");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "guff failed: {:?}\nstdout={stdout}\nstderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    let mut files: Vec<String> = stdout
        .lines()
        .filter(|l| l.contains(".go:"))
        .map(|l| l.split(".go:").next().unwrap().to_string() + ".go")
        .collect();
    files.sort();
    files
}

#[test]
fn dot_dot_dot_from_a_subdirectory_is_that_directory() {
    let root = module("scope", None);
    assert_eq!(files(&root, "svc", "./..."), ["svc/app/app.go"]);
    assert_eq!(files(&root, "svc/app", "./..."), ["svc/app/app.go"]);
    assert_eq!(
        files(&root, ".", "./..."),
        ["lib/lib.go", "other/other.go", "svc/app/app.go"]
    );
    assert_eq!(files(&root, ".", "./svc/..."), ["svc/app/app.go"]);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn printed_paths_follow_relative_path_mode() {
    // `cfg` and unset: the config's directory, the module root here.
    for mode in [None, Some("cfg"), Some("gomod")] {
        let root = module("mode", mode);
        assert_eq!(files(&root, "svc", "./..."), ["svc/app/app.go"], "{mode:?}");
        let _ = fs::remove_dir_all(&root);
    }
    // `wd`: the working directory.
    let root = module("wd", Some("wd"));
    assert_eq!(files(&root, "svc", "./..."), ["app/app.go"]);
    assert_eq!(files(&root, "svc/app", "./..."), ["app.go"]);
    let _ = fs::remove_dir_all(&root);
}
