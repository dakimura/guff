//! `issues.new*` must not swallow files git does not track yet.
//!
//! Regression test for the 2026-08-17 field report (issue A). With
//! `issues.new-from-merge-base` set, guff reported **nothing** for a brand-new
//! file that had not been `git add`ed, and exited 0. golangci-lint reports such
//! a file in full: revgrep collects `git ls-files --others --exclude-standard`
//! and treats every line of those files as new (`patch.go`), independently of
//! `whole-files`. Any pre-commit or CI flow that lints before staging was
//! therefore letting new files through completely unchecked — and doing it in
//! the direction that looks clean.
//!
//! The unit tests in `diff.rs` cover `DiffState::keeps` on a hand-built state;
//! they would still pass if nothing ever populated `new_files`. This one drives
//! the real binary against a real git repository, so it fails if the
//! `git ls-files` call is dropped, mis-parsed, or run from the wrong directory.
//!
//! Each case carries a *tracked, unmodified* file with a finding of its own, so
//! the assertion discriminates: a diff filter that has stopped filtering
//! altogether reports both files and fails here just as loudly as the bug does.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// Committed before the base revision and never touched again: its finding must
/// stay filtered out.
const TRACKED: &str = r#"package tracked

import "os"

func Tracked() { os.Remove("/tmp/tracked") }
"#;

/// Never added to git. Every line of it is new.
const UNTRACKED: &str = r#"package untracked

import "os"

func Untracked() { os.Remove("/tmp/untracked") }
"#;

const CONFIG_HEAD: &str = r#"version: "2"
linters:
  default: none
  enable:
    - errcheck
issues:
"#;

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        // Keep the fixture independent of the developer's global git config
        // (user identity, commit signing, `init.defaultBranch`, hooks).
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "guff test")
        .env("GIT_AUTHOR_EMAIL", "guff@example.com")
        .env("GIT_COMMITTER_NAME", "guff test")
        .env("GIT_COMMITTER_EMAIL", "guff@example.com")
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A repo whose committed state is `tracked/` alone, with `origin/main` naming
/// that commit, plus an untracked `untracked/` in the working tree.
fn init_repo(root: &Path, issues_config: &str) {
    fs::write(root.join("go.mod"), "module example.com/diffnew\n\ngo 1.22\n").unwrap();
    fs::write(root.join(".golangci.yml"), format!("{CONFIG_HEAD}{issues_config}")).unwrap();
    fs::create_dir_all(root.join("tracked")).unwrap();
    fs::write(root.join("tracked/tracked.go"), TRACKED).unwrap();

    git(root, &["init", "--quiet", "--initial-branch=main"]);
    git(root, &["add", "go.mod", ".golangci.yml", "tracked/tracked.go"]);
    git(root, &["commit", "--quiet", "--no-gpg-sign", "-m", "base"]);
    // A local ref literally named `origin/main`, so `merge-base HEAD origin/main`
    // resolves without a network remote.
    git(root, &["branch", "--force", "origin/main", "HEAD"]);

    // Written after the commit: git has never heard of this path.
    fs::create_dir_all(root.join("untracked")).unwrap();
    fs::write(root.join("untracked/untracked.go"), UNTRACKED).unwrap();
}

fn run_guff(root: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_guff"))
        .current_dir(root)
        .args(["run", "--issues-exit-code", "0", "--no-cache", "./..."])
        .output()
        .expect("run guff");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "guff exited {:?}\nstdout:\n{stdout}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
    stdout
}

fn findings_in(output: &str, dir: &str) -> Vec<String> {
    output
        .lines()
        .filter(|l| l.starts_with(&format!("{dir}/")) && l.contains("(errcheck)"))
        .map(str::to_string)
        .collect()
}

fn assert_only_untracked_reported(stdout: &str, case: &str) {
    assert_eq!(
        findings_in(stdout, "untracked").len(),
        1,
        "[{case}] the untracked file's finding was dropped — this is the \
         silent-clean-exit bug the field report hit\n{stdout}"
    );
    assert!(
        findings_in(stdout, "tracked").is_empty(),
        "[{case}] the unchanged tracked file leaked through, so the diff \
         filter is not filtering at all\n{stdout}"
    );
}

/// The field report's exact shape: `new-from-merge-base` + `whole-files`.
#[test]
#[ignore = "requires go and git on PATH; run with cargo test -p guff-lint -- --ignored"]
fn merge_base_whole_files_reports_untracked_file() {
    let tmp = TempDir::new().unwrap();
    init_repo(
        tmp.path(),
        "  new-from-merge-base: origin/main\n  whole-files: true\n",
    );
    assert_only_untracked_reported(&run_guff(tmp.path()), "merge-base + whole-files");
}

/// Same, without `whole-files`. revgrep stores an untracked file with a `nil`
/// line list and `IsNew` reads `nil` as "all lines", so line mode must report it
/// too — a fix that only special-cased `whole-files` would pass the test above
/// and fail this one.
#[test]
#[ignore = "requires go and git on PATH; run with cargo test -p guff-lint -- --ignored"]
fn merge_base_line_mode_reports_untracked_file() {
    let tmp = TempDir::new().unwrap();
    init_repo(tmp.path(), "  new-from-merge-base: origin/main\n");
    assert_only_untracked_reported(&run_guff(tmp.path()), "merge-base, line mode");
}

/// `new-from-rev` goes through the same revgrep branch (`revisionTo` empty), so
/// it collects untracked files as well.
#[test]
#[ignore = "requires go and git on PATH; run with cargo test -p guff-lint -- --ignored"]
fn new_from_rev_reports_untracked_file() {
    let tmp = TempDir::new().unwrap();
    init_repo(tmp.path(), "  new-from-rev: HEAD\n");
    assert_only_untracked_reported(&run_guff(tmp.path()), "new-from-rev");
}

/// `issues.new` (working-tree mode) likewise.
#[test]
#[ignore = "requires go and git on PATH; run with cargo test -p guff-lint -- --ignored"]
fn new_reports_untracked_file() {
    let tmp = TempDir::new().unwrap();
    init_repo(tmp.path(), "  new: true\n");
    assert_only_untracked_reported(&run_guff(tmp.path()), "issues.new");
}

/// Staging the file must not change the answer. This is the half the report
/// observed working (`git add -N` made guff agree with golangci-lint); pinning
/// it keeps a future fix from trading one direction for the other.
#[test]
#[ignore = "requires go and git on PATH; run with cargo test -p guff-lint -- --ignored"]
fn staged_new_file_is_still_reported() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    init_repo(
        root,
        "  new-from-merge-base: origin/main\n  whole-files: true\n",
    );
    git(root, &["add", "-N", "untracked/untracked.go"]);
    assert_only_untracked_reported(&run_guff(root), "after git add -N");
}

/// An explicit `new-from-patch` describes the change set on its own: revgrep's
/// `loadPatch` returns before it ever asks git for untracked files, so guff must
/// not add them either.
///
/// The patch names `tracked/tracked.go` — the *opposite* file — so the case
/// discriminates. A run that reported nothing at all (a broken patch parser, a
/// crash swallowed by the exit code) would fail on the first assertion, and one
/// that still folded in untracked files would fail on the second.
#[test]
#[ignore = "requires go and git on PATH; run with cargo test -p guff-lint -- --ignored"]
fn new_from_patch_does_not_pick_up_untracked_files() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    init_repo(root, "  new-from-patch: change.patch\n  whole-files: true\n");
    fs::write(
        root.join("change.patch"),
        "diff --git a/tracked/tracked.go b/tracked/tracked.go\n\
         --- a/tracked/tracked.go\n\
         +++ b/tracked/tracked.go\n\
         @@ -5,0 +5,1 @@\n\
         +func Tracked() { os.Remove(\"/tmp/tracked\") }\n",
    )
    .unwrap();

    let stdout = run_guff(root);
    assert_eq!(
        findings_in(&stdout, "tracked").len(),
        1,
        "the patch names tracked/tracked.go, so its finding must survive\n{stdout}"
    );
    assert!(
        findings_in(&stdout, "untracked").is_empty(),
        "a supplied patch is the whole change set — guff must not fold git's \
         untracked files into it\n{stdout}"
    );
}
