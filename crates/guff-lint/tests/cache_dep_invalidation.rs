//! A warm issues cache must re-analyze a package when only its *dependency*
//! changed.
//!
//! `guff-runner`'s `changed_file_only_that_package_misses` already covers
//! per-package granularity, but it uses two packages that do not import each
//! other, so it says nothing about propagation. This is the property that
//! decides whether a cache carried between CI runs is safe: if a package's
//! cache entry were keyed on its own contents alone, editing an exported
//! signature would leave every caller reporting the stale pre-edit result, and
//! a real finding would disappear from the run that introduced it.
//!
//! The module has a third package that imports nothing, so the assertion can
//! discriminate rather than merely observe. After `dep` changes, `unrelated`
//! must *hit* and `user` must *miss*. A cache that silently stopped working
//! would give two misses and fail; a cache that ignored dependencies would give
//! two hits and fail. Only the correct behaviour passes.
//!
//! End-to-end through the binary rather than through `IssueCache` directly,
//! because the property has to survive `go list`, the package graph and the
//! whole pipeline — not just the hashing function.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// `dep.Do` returns nothing, so `user.Use` ignoring it is clean.
const DEP_CLEAN: &str = r#"package dep

func Do() {}
"#;

/// `dep.Do` now returns an error that `user.Use` drops on the floor: errcheck
/// must fire in `user`, whose source never changed.
const DEP_RETURNS_ERROR: &str = r#"package dep

import "errors"

func Do() error { return errors.New("boom") }
"#;

/// Never rewritten. Its findings are a pure function of `dep`.
const USER: &str = r#"package user

import "example.com/depinvalidation/dep"

func Use() {
	dep.Do()
}
"#;

/// Imports nothing and never changes: the control that proves the cache is
/// still live when `user` misses.
const UNRELATED: &str = r#"package unrelated

func Compute() int { return 1 }
"#;

fn write_module(root: &Path) {
    fs::write(
        root.join("go.mod"),
        "module example.com/depinvalidation\n\ngo 1.22\n",
    )
    .unwrap();
    for (dir, file, body) in [
        ("dep", "dep.go", DEP_CLEAN),
        ("user", "user.go", USER),
        ("unrelated", "unrelated.go", UNRELATED),
    ] {
        fs::create_dir_all(root.join(dir)).unwrap();
        fs::write(root.join(dir).join(file), body).unwrap();
    }
}

struct Run {
    stdout: String,
    hits: u32,
    misses: u32,
}

fn run_guff(root: &Path, cache: &Path, extra: &[&str]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_guff"));
    cmd.current_dir(root)
        .env("GUFF_CACHE", cache)
        .env("GUFF_DEBUG_CACHE", "1")
        .args(["run", "--no-config", "--default", "standard"])
        .args(["--issues-exit-code", "0"])
        .args(extra)
        .arg("./...");
    let out = cmd.output().expect("run guff");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "guff exited {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code(),
    );

    // `guff: cache hits=N misses=M (...)`, emitted under GUFF_DEBUG_CACHE.
    let (hits, misses) = stderr
        .lines()
        .rev()
        .find_map(parse_cache_line)
        .unwrap_or_else(|| panic!("no cache summary in stderr:\n{stderr}"));

    Run {
        stdout,
        hits,
        misses,
    }
}

fn parse_cache_line(line: &str) -> Option<(u32, u32)> {
    let rest = line.split_once("cache hits=")?.1;
    let (hits, rest) = rest.split_once(' ')?;
    let misses = rest.strip_prefix("misses=")?;
    let misses = misses.split_whitespace().next()?;
    Some((hits.parse().ok()?, misses.parse().ok()?))
}

fn user_findings(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter(|l| l.starts_with("user/user.go:"))
        .collect()
}

#[test]
#[ignore = "requires go on PATH; run with cargo test -p guff-lint -- --ignored"]
fn warm_cache_reanalyzes_dependents_of_a_changed_package() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let cache = root.join("guff-cache");
    fs::create_dir_all(&cache).unwrap();
    write_module(root);

    // Populate the cache with `user` reported clean.
    let cold = run_guff(root, &cache, &[]);
    assert_eq!(
        (cold.hits, cold.misses),
        (0, 3),
        "an empty cache should miss every package"
    );
    assert!(
        user_findings(&cold.stdout).is_empty(),
        "expected no finding in user before the dependency changed, got:\n{}",
        cold.stdout
    );

    // Change the dependency only. `user/user.go` is untouched, so a cache keyed
    // on package-local content alone would serve the clean entry above.
    let user_before = fs::read(root.join("user/user.go")).unwrap();
    fs::write(root.join("dep/dep.go"), DEP_RETURNS_ERROR).unwrap();
    assert_eq!(
        user_before,
        fs::read(root.join("user/user.go")).unwrap(),
        "the test itself must not touch user/user.go"
    );

    let warm = run_guff(root, &cache, &[]);

    // The discriminating assertion. `unrelated` hits, so the cache is live;
    // `dep` and `user` both miss, so the change propagated across the import.
    assert_eq!(
        (warm.hits, warm.misses),
        (1, 2),
        "expected unrelated to hit and dep+user to miss; a dead cache would be \
         (0, 3) and a dependency-blind one (2, 1)"
    );

    let warm_findings = user_findings(&warm.stdout);
    assert_eq!(
        warm_findings.len(),
        1,
        "warm cache should surface the new errcheck finding in the dependent \
         package; got:\n{}",
        warm.stdout
    );
    assert!(
        warm_findings[0].contains("(errcheck)"),
        "unexpected finding: {}",
        warm_findings[0]
    );

    // Ground truth: the same tree with the cache bypassed entirely. The cached
    // run must be indistinguishable from it, not merely non-empty.
    let uncached = run_guff(root, &cache, &["--no-cache"]);
    assert_eq!(
        user_findings(&warm.stdout),
        user_findings(&uncached.stdout),
        "warm-cache output diverged from the uncached run\nwarm:\n{}\nuncached:\n{}",
        warm.stdout,
        uncached.stdout
    );
}
