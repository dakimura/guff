//! Integration: native list against a planted GOMODCACHE + local replace.

use std::fs;
use std::path::PathBuf;

use guff_golist::{list_packages, BailReason, ListConfig};

fn write(path: &PathBuf, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

#[test]
fn lists_module_from_fake_gomodcache() {
    let tmp = std::env::temp_dir().join(format!(
        "guff-golist-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    let main = tmp.join("main");
    let cache = tmp.join("modcache");
    write(
        &main.join("go.mod"),
        "module example.com/app\n\ngo 1.22\n\nrequire example.com/lib v1.0.0\n",
    );
    write(
        &main.join("main.go"),
        "package main\n\nimport (\n\t\"fmt\"\n\t\"example.com/lib\"\n)\n\nfunc main() { fmt.Println(lib.V) }\n",
    );

    let lib_root = cache.join("example.com").join("lib@v1.0.0");
    write(
        &lib_root.join("go.mod"),
        "module example.com/lib\n\ngo 1.22\n",
    );
    write(&lib_root.join("lib.go"), "package lib\n\nconst V = 1\n");

    let cfg = ListConfig {
        dir: main.clone(),
        need_deps: true,
        gomodcache: Some(cache),
        ..ListConfig::default()
    };
    let resp = list_packages(&cfg, &[".".to_string()]).expect("list");
    assert!(resp.roots.iter().any(|r| r == "example.com/app"));
    assert!(
        resp.packages.iter().any(|p| p.id == "example.com/lib"),
        "expected example.com/lib from fake GOMODCACHE; pkgs={:?}",
        resp.packages.iter().map(|p| &p.id).collect::<Vec<_>>()
    );
    assert!(resp.packages.iter().any(|p| p.id == "fmt"));

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn lists_local_replace() {
    let tmp = std::env::temp_dir().join(format!(
        "guff-golist-replace-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    let main = tmp.join("main");
    let dep = tmp.join("dep");
    write(
        &main.join("go.mod"),
        "module example.com/app\n\ngo 1.22\n\nrequire example.com/dep v0.0.0\n\nreplace example.com/dep => ../dep\n",
    );
    write(
        &main.join("main.go"),
        "package main\n\nimport \"example.com/dep\"\n\nfunc main() { _ = dep.Hello }\n",
    );
    write(&dep.join("go.mod"), "module example.com/dep\n\ngo 1.22\n");
    write(&dep.join("dep.go"), "package dep\n\nconst Hello = \"hi\"\n");

    let cfg = ListConfig {
        dir: main,
        need_deps: true,
        gomodcache: Some(tmp.join("empty-cache")),
        ..ListConfig::default()
    };
    let resp = list_packages(&cfg, &[".".to_string()]).expect("list with replace");
    assert!(resp.packages.iter().any(|p| p.id == "example.com/dep"));

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn bails_on_old_go_version() {
    let tmp = std::env::temp_dir().join(format!(
        "guff-golist-old-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    write(&tmp.join("go.mod"), "module example.com/old\n\ngo 1.16\n");
    write(&tmp.join("main.go"), "package main\n\nfunc main() {}\n");

    let cfg = ListConfig {
        dir: tmp.clone(),
        ..ListConfig::default()
    };
    let err = list_packages(&cfg, &[".".to_string()]).unwrap_err();
    assert_eq!(err.reason, BailReason::GoVersionTooOld);

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn lists_test_variants_like_go_list() {
    let tmp = std::env::temp_dir().join(format!(
        "guff-golist-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    write(
        &tmp.join("go.mod"),
        "module example.com/foo\n\ngo 1.22\n",
    );
    write(&tmp.join("foo.go"), "package foo\n\nfunc F() int { return 1 }\n");
    write(
        &tmp.join("foo_test.go"),
        "package foo\n\nimport \"testing\"\n\nfunc TestF(t *testing.T) { if F() != 1 { t.Fatal() } }\n",
    );
    write(
        &tmp.join("foo_ext_test.go"),
        "package foo_test\n\nimport (\n\t\"testing\"\n\t\"example.com/foo\"\n)\n\nfunc TestExt(t *testing.T) { if foo.F() != 1 { t.Fatal() } }\n",
    );

    let cfg = ListConfig {
        dir: tmp.clone(),
        tests: true,
        need_deps: true,
        gomodcache: Some(tmp.join("empty-cache")),
        ..ListConfig::default()
    };
    let resp = list_packages(&cfg, &[".".to_string()]).expect("list with tests");

    let ids: Vec<_> = resp.packages.iter().map(|p| p.id.as_str()).collect();
    assert!(ids.contains(&"example.com/foo"), "plain: {ids:?}");
    assert!(
        ids.contains(&"example.com/foo [example.com/foo.test]"),
        "internal variant: {ids:?}"
    );
    assert!(
        ids.contains(&"example.com/foo_test [example.com/foo.test]"),
        "external variant: {ids:?}"
    );
    assert!(
        ids.contains(&"example.com/foo.test"),
        "testmain: {ids:?}"
    );

    let plain = resp
        .packages
        .iter()
        .find(|p| p.id == "example.com/foo")
        .unwrap();
    assert_eq!(plain.compiled_go_files.len(), 1, "plain stays prod-only");
    assert!(plain.for_test.is_empty());

    let internal = resp
        .packages
        .iter()
        .find(|p| p.id == "example.com/foo [example.com/foo.test]")
        .unwrap();
    assert_eq!(internal.pkg_path, "example.com/foo");
    assert_eq!(internal.for_test, "example.com/foo");
    assert_eq!(internal.compiled_go_files.len(), 2);

    let external = resp
        .packages
        .iter()
        .find(|p| p.id == "example.com/foo_test [example.com/foo.test]")
        .unwrap();
    assert_eq!(external.pkg_path, "example.com/foo_test");
    assert_eq!(external.for_test, "example.com/foo");
    let foo_import = external
        .imports
        .iter()
        .find(|(src, _)| src == "example.com/foo")
        .expect("imports foo");
    assert_eq!(
        foo_import.1, "example.com/foo [example.com/foo.test]",
        "xtest must import the internal test variant"
    );

    assert!(resp.roots.iter().any(|r| r == "example.com/foo"));
    assert!(resp
        .roots
        .iter()
        .any(|r| r == "example.com/foo [example.com/foo.test]"));
    assert!(resp
        .roots
        .iter()
        .any(|r| r == "example.com/foo_test [example.com/foo.test]"));
    assert!(resp.roots.iter().any(|r| r == "example.com/foo.test"));

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn lists_fortest_dep_variants() {
    let tmp = std::env::temp_dir().join(format!(
        "guff-golist-fortest-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("a")).unwrap();
    fs::create_dir_all(tmp.join("b")).unwrap();

    write(
        &tmp.join("go.mod"),
        "module example.com/mod\n\ngo 1.22\n",
    );
    write(&tmp.join("a/a.go"), "package a\n\nfunc A() int { return 1 }\n");
    write(
        &tmp.join("a/a_test.go"),
        "package a\n\nimport \"testing\"\n\nfunc TestA(t *testing.T) {}\n",
    );
    write(
        &tmp.join("a/a_ext_test.go"),
        "package a_test\n\nimport (\n\t\"testing\"\n\t\"example.com/mod/a\"\n\t\"example.com/mod/b\"\n)\n\nfunc TestExt(t *testing.T) { _ = a.A(); _ = b.B() }\n",
    );
    // b imports a → must be recompiled as b [a.test] for a's test binary.
    write(
        &tmp.join("b/b.go"),
        "package b\n\nimport \"example.com/mod/a\"\n\nfunc B() int { return a.A() }\n",
    );

    let cfg = ListConfig {
        dir: tmp.clone(),
        tests: true,
        need_deps: true,
        gomodcache: Some(tmp.join("empty-cache")),
        ..ListConfig::default()
    };
    let resp = list_packages(&cfg, &["./a".to_string()]).expect("list");
    let ids: Vec<_> = resp.packages.iter().map(|p| p.id.as_str()).collect();
    assert!(
        ids.contains(&"example.com/mod/b [example.com/mod/a.test]"),
        "missing for-test dep variant: {ids:?}"
    );
    let b_var = resp
        .packages
        .iter()
        .find(|p| p.id == "example.com/mod/b [example.com/mod/a.test]")
        .unwrap();
    assert!(b_var.dep_only);
    assert_eq!(b_var.for_test, "example.com/mod/a");
    let a_imp = b_var
        .imports
        .iter()
        .find(|(src, _)| src == "example.com/mod/a")
        .unwrap();
    assert_eq!(a_imp.1, "example.com/mod/a [example.com/mod/a.test]");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn lists_from_vendor_modules_txt() {
    let tmp = std::env::temp_dir().join(format!(
        "guff-golist-vendor-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    let main = tmp.join("main");
    write(
        &main.join("go.mod"),
        "module example.com/app\n\ngo 1.22\n\nrequire example.com/lib v1.0.0\n",
    );
    write(
        &main.join("main.go"),
        "package main\n\nimport (\n\t\"fmt\"\n\t\"example.com/lib\"\n)\n\nfunc main() { fmt.Println(lib.V) }\n",
    );

    // Plant vendor/ instead of GOMODCACHE — empty cache must still resolve.
    write(
        &main.join("vendor/modules.txt"),
        "# example.com/lib v1.0.0\n## explicit; go 1.22\nexample.com/lib\n",
    );
    write(
        &main.join("vendor/example.com/lib/go.mod"),
        "module example.com/lib\n\ngo 1.22\n",
    );
    write(
        &main.join("vendor/example.com/lib/lib.go"),
        "package lib\n\nconst V = 1\n",
    );

    let cfg = ListConfig {
        dir: main.clone(),
        need_deps: true,
        gomodcache: Some(tmp.join("empty-cache")),
        ..ListConfig::default()
    };
    let resp = list_packages(&cfg, &[".".to_string()]).expect("list with vendor/");
    let lib = resp
        .packages
        .iter()
        .find(|p| p.id == "example.com/lib")
        .expect("example.com/lib from vendor/");
    assert!(
        lib.dir.ends_with("vendor/example.com/lib"),
        "dir should be under vendor/: {}",
        lib.dir.display()
    );
    let m = lib.module.as_ref().expect("module metadata");
    assert_eq!(m.path, "example.com/lib");
    assert_eq!(m.version, "v1.0.0");
    assert_eq!(m.go_version, "1.22");
    assert!(m.dir.as_os_str().is_empty(), "go list omits Module.Dir when vendored");
    assert!(resp.packages.iter().any(|p| p.id == "fmt"));

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn bails_on_vendor_without_modules_txt() {
    let tmp = std::env::temp_dir().join(format!(
        "guff-golist-vendor-bail-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    write(&tmp.join("go.mod"), "module example.com/app\n\ngo 1.22\n");
    write(&tmp.join("main.go"), "package main\n\nfunc main() {}\n");
    fs::create_dir_all(tmp.join("vendor")).unwrap();

    let cfg = ListConfig {
        dir: tmp.clone(),
        ..ListConfig::default()
    };
    let err = list_packages(&cfg, &[".".to_string()]).unwrap_err();
    assert_eq!(err.reason, BailReason::Vendor);

    let _ = fs::remove_dir_all(&tmp);
}

/// `go list -test` reports the same `IgnoredGoFiles` on `P [P.test]` as on `P`.
///
/// Build constraints do not change because test files joined the package, and
/// analyzers ask about them: modernize's `atomictypes` refuses to rewrite a
/// package-level var or a struct field when the package has source it cannot
/// see. Emptying the field on the test variant meant every package that has
/// tests — which is most of them — looked as though nothing was excluded, and
/// coredns's `plugin/forward` and `plugin/grpc` (each with a
/// `//go:build gofuzz` file) were guff-only findings because of it.
///
/// The external test package is genuinely empty here: `go list` reports no
/// `IgnoredGoFiles` for `P_test [P.test]`.
#[test]
fn test_variant_keeps_the_packages_ignored_files() {
    let tmp = std::env::temp_dir().join(format!(
        "guff-golist-ignored-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    write(&tmp.join("go.mod"), "module example.com/foo\n\ngo 1.22\n");
    write(&tmp.join("foo.go"), "package foo\n\nfunc F() int { return 1 }\n");
    write(
        &tmp.join("foo_test.go"),
        "package foo\n\nimport \"testing\"\n\nfunc TestF(t *testing.T) { if F() != 1 { t.Fatal() } }\n",
    );
    write(
        &tmp.join("foo_ext_test.go"),
        "package foo_test\n\nimport (\n\t\"testing\"\n\t\"example.com/foo\"\n)\n\nfunc TestExt(t *testing.T) { if foo.F() != 1 { t.Fatal() } }\n",
    );
    write(
        &tmp.join("fuzz.go"),
        "//go:build gofuzz\n\npackage foo\n\nfunc Fuzz(data []byte) int { return 0 }\n",
    );

    let cfg = ListConfig {
        dir: tmp.clone(),
        tests: true,
        need_deps: true,
        gomodcache: Some(tmp.join("empty-cache")),
        ..ListConfig::default()
    };
    let resp = list_packages(&cfg, &[".".to_string()]).expect("list with tests");

    let ignored_of = |id: &str| -> Vec<String> {
        resp.packages
            .iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("no package {id}"))
            .ignored_files
            .iter()
            .map(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string()
            })
            .collect()
    };

    assert_eq!(ignored_of("example.com/foo"), vec!["fuzz.go".to_string()]);
    assert_eq!(
        ignored_of("example.com/foo [example.com/foo.test]"),
        vec!["fuzz.go".to_string()],
        "the internal test variant carries the same excluded files"
    );
    assert!(
        ignored_of("example.com/foo_test [example.com/foo.test]").is_empty(),
        "the external test package has none, as go list reports"
    );

    let _ = fs::remove_dir_all(&tmp);
}

/// `./...` does not match a directory whose base name starts with `.` or `_`,
/// nor one named `testdata` — but naming it explicitly still resolves it.
///
/// `go list`'s own rule (`cmd/go/internal/search`, `MatchPackages`):
///
/// ```go
/// if !top && (strings.HasPrefix(elem, ".") || strings.HasPrefix(elem, "_") || elem == "testdata") {
///     return filepath.SkipDir
/// }
/// ```
///
/// The `_` half was missing here, so `./...` on pyroscope reached
/// `examples/_templates` — a package upstream's `./...` never sees, and
/// therefore one with nothing to compare guff's three goconst findings
/// against. Measured against `go list` 1.26.5 on the same layout: `./...`
/// returns `keep` only, `./_skip/` returns the package.
#[test]
fn wildcard_skips_underscore_dot_and_testdata_dirs() {
    let tmp = std::env::temp_dir().join(format!(
        "guff-golist-wildcard-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    let main = tmp.join("main");
    write(&main.join("go.mod"), "module example.com/app\n\ngo 1.22\n");
    for dir in ["keep", "_skip", ".hidden", "testdata", "vendored"] {
        write(
            &main.join(dir).join("a.go"),
            &format!("package {}\n\nconst V = 1\n", dir.trim_start_matches(['_', '.'])),
        );
    }

    let cfg = ListConfig {
        dir: main.clone(),
        ..ListConfig::default()
    };
    let resp = list_packages(&cfg, &["./...".to_string()]).expect("list");
    let mut roots = resp.roots.clone();
    roots.sort();
    assert_eq!(
        roots,
        vec![
            "example.com/app/keep".to_string(),
            "example.com/app/vendored".to_string()
        ],
        "`./...` must skip _skip, .hidden and testdata"
    );

    // Named explicitly, the same directory is an ordinary package — this is
    // the half a blanket "never walk into `_`" would break.
    let resp = list_packages(&cfg, &["./_skip/".to_string()]).expect("list _skip");
    assert_eq!(resp.roots, vec!["example.com/app/_skip".to_string()]);

    let _ = fs::remove_dir_all(&tmp);
}

/// An import cycle is left to `go list`: which package carries the error and
/// the `ImportStack` it prints follow `cmd/go`'s load order. The test-only
/// form (`b`'s in-package test imports `a`, which imports `b`) is a cycle only
/// among the `[b.test]` variants, so it bails only with tests on.
#[test]
fn bails_on_an_import_cycle() {
    let tmp = std::env::temp_dir().join(format!(
        "guff-golist-cycle-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    let module = |name: &str, files: &[(&str, &str)]| {
        let dir = tmp.join(name);
        write(&dir.join("go.mod"), "module ex\n\ngo 1.22\n");
        for (path, body) in files {
            write(&dir.join(path), body);
        }
        dir
    };
    let list = |dir: &PathBuf, tests: bool| {
        let cfg = ListConfig {
            dir: dir.clone(),
            tests,
            ..ListConfig::default()
        };
        list_packages(&cfg, &["./...".to_string()])
    };

    let pair = module(
        "pair",
        &[
            ("a/a.go", "package a\n\nimport _ \"ex/b\"\n"),
            ("b/b.go", "package b\n\nimport _ \"ex/a\"\n"),
        ],
    );
    let selfimp = module("self", &[("a/a.go", "package a\n\nimport _ \"ex/a\"\n")]);
    let three = module(
        "three",
        &[
            ("a/a.go", "package a\n\nimport _ \"ex/b\"\n"),
            ("b/b.go", "package b\n\nimport _ \"ex/c\"\n"),
            ("c/c.go", "package c\n\nimport _ \"ex/a\"\n"),
        ],
    );
    for dir in [&pair, &selfimp, &three] {
        for tests in [false, true] {
            let err = list(dir, tests).unwrap_err();
            assert_eq!(err.reason, BailReason::ImportCycle, "{dir:?} tests={tests}");
        }
    }

    let in_test = module(
        "intest",
        &[
            ("a/a.go", "package a\n\nimport _ \"ex/b\"\n"),
            ("b/b.go", "package b\n"),
            (
                "b/b_test.go",
                "package b\n\nimport (\n\t\"testing\"\n\n\t_ \"ex/a\"\n)\n\nfunc TestB(t *testing.T) {}\n",
            ),
        ],
    );
    assert!(list(&in_test, false).is_ok(), "no cycle without the test variant");
    assert_eq!(
        list(&in_test, true).unwrap_err().reason,
        BailReason::ImportCycle
    );

    // An external test package importing its own package, and a test variant
    // of a package another root imports, are not cycles.
    let xtest = module(
        "xtest",
        &[
            ("a/a.go", "package a\n\nimport _ \"ex/b\"\n"),
            ("b/b.go", "package b\n"),
            (
                "b/b_test.go",
                "package b\n\nimport \"testing\"\n\nfunc TestB(t *testing.T) {}\n",
            ),
            (
                "b/x_test.go",
                "package b_test\n\nimport (\n\t\"testing\"\n\n\t_ \"ex/a\"\n\t_ \"ex/b\"\n)\n\nfunc TestX(t *testing.T) {}\n",
            ),
        ],
    );
    assert!(list(&xtest, true).is_ok());
    assert!(list(&xtest, false).is_ok());

    let _ = fs::remove_dir_all(&tmp);
}

/// Directories whose only buildable files are `*_test.go`, next to the shapes
/// that must stay out. Every expectation is what `go list [-test] ./...`
/// (go1.27.1) printed for this exact tree:
///
/// - with or without `-test`, `t1`..`t4` are listed — no GoFiles, no error
///   (go/build's NoGoError counts test files too). The native lister scanned
///   roots without opening tests when `tests: false`, called them empty, and
///   dropped them (caddy `caddytest/integration`).
/// - `t5` (everything tagged out), `t6` (only a tagged-out test) and `t7`
///   (no `.go` file) are never listed.
/// - `P_test [P.test]` imports only what its sources name: no implicit `P`.
/// - the testmain of an xtest-only package imports plain `P` only when `P`
///   has files (`x1.test` does, `t2.test` does not).
#[test]
fn test_only_directories_are_listed_like_go_list() {
    let tmp = std::env::temp_dir().join(format!(
        "guff-golist-testonly-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    let internal = |p: &str| {
        format!("package {p}\n\nimport \"testing\"\n\nfunc TestX(t *testing.T) {{}}\n")
    };
    let external = |p: &str| {
        format!("package {p}_test\n\nimport \"testing\"\n\nfunc TestY(t *testing.T) {{}}\n")
    };
    write(&tmp.join("go.mod"), "module example.com/m\n\ngo 1.22\n");
    write(&tmp.join("t1/x_test.go"), &internal("t1"));
    write(&tmp.join("t2/y_test.go"), &external("t2"));
    write(&tmp.join("t3/x_test.go"), &internal("t3"));
    write(&tmp.join("t3/y_test.go"), &external("t3"));
    write(&tmp.join("t4/a.go"), "//go:build never\n\npackage t4\n");
    write(&tmp.join("t4/x_test.go"), &internal("t4"));
    write(&tmp.join("t5/a.go"), "//go:build never\n\npackage t5\n");
    write(
        &tmp.join("t5/x_test.go"),
        &format!("//go:build never\n\n{}", internal("t5")),
    );
    write(
        &tmp.join("t6/x_test.go"),
        &format!("//go:build never\n\n{}", internal("t6")),
    );
    write(&tmp.join("t7/README"), "not go\n");
    write(&tmp.join("x1/a.go"), "package x1\n\nfunc A() {}\n");
    write(&tmp.join("x1/y_test.go"), &external("x1"));
    write(&tmp.join("x2/a.go"), "package x2\n\nfunc A() {}\n");
    write(
        &tmp.join("x2/y_test.go"),
        "package x2_test\n\nimport (\n\t\"testing\"\n\t\"example.com/m/x2\"\n)\n\nfunc TestY(t *testing.T) { x2.A() }\n",
    );

    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    let m = |s: &str| format!("example.com/m/{s}");

    // `tests: false` — `go list ./...`.
    let cfg = ListConfig {
        dir: tmp.clone(),
        tests: false,
        need_deps: true,
        gomodcache: Some(tmp.join("empty-cache")),
        ..ListConfig::default()
    };
    let resp = list_packages(&cfg, &["./...".to_string()]).expect("list");
    assert_eq!(
        sorted(resp.roots.clone()),
        ["t1", "t2", "t3", "t4", "x1", "x2"].map(m).to_vec()
    );
    let t1 = resp.packages.iter().find(|p| p.id == m("t1")).unwrap();
    assert!(t1.go_files.is_empty() && t1.imports.is_empty() && t1.errors.is_empty());
    assert!(
        !resp.packages.iter().any(|p| p.id.contains(".test")),
        "no test variants without tests"
    );

    // `tests: true` — `go list -test ./...`.
    let cfg = ListConfig {
        tests: true,
        ..cfg
    };
    let resp = list_packages(&cfg, &["./...".to_string()]).expect("list -test");
    let mut want: Vec<String> = ["t1", "t2", "t3", "t4", "x1", "x2"].map(m).to_vec();
    for p in ["t1", "t2", "t3", "t4", "x1", "x2"] {
        want.push(m(&format!("{p}.test")));
    }
    for p in ["t1", "t3", "t4"] {
        want.push(format!("{} [{}.test]", m(p), m(p)));
    }
    for p in ["t2", "t3", "x1", "x2"] {
        want.push(format!("{}_test [{}.test]", m(p), m(p)));
    }
    assert_eq!(sorted(resp.roots.clone()), sorted(want));

    let imports_of = |id: &str| -> Vec<String> {
        let pkg = resp
            .packages
            .iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("{id} listed"));
        sorted(pkg.imports.iter().map(|(src, _)| src.clone()).collect())
    };
    let t4v = format!("{} [{}.test]", m("t4"), m("t4"));
    let t4_files = &resp.packages.iter().find(|p| p.id == t4v).unwrap().go_files;
    assert_eq!(t4_files.len(), 1, "t4 [t4.test] is the test file alone");
    for p in ["t2", "t3", "x1"] {
        assert_eq!(
            imports_of(&format!("{}_test [{}.test]", m(p), m(p))),
            vec!["testing".to_string()],
            "{p}_test imports only what it names"
        );
    }
    assert_eq!(
        imports_of(&format!("{}_test [{}.test]", m("x2"), m("x2"))),
        vec![m("x2"), "testing".to_string()]
    );
    assert!(
        imports_of(&m("x1.test")).contains(&m("x1")),
        "xtest-only testmain links plain P when P has files"
    );
    assert!(
        !imports_of(&m("t2.test")).contains(&m("t2")),
        "and not when P has none"
    );

    let _ = fs::remove_dir_all(&tmp);
}

/// The linker's implicit deps of a command (`LinkerDeps` in cmd/go's
/// load/pkg.go). Measured against `go list -deps -test -json` (go1.27.1,
/// darwin/arm64): a `package main` that imports nothing still has the whole
/// `runtime` closure in Deps, never in Imports; `P [P.test]` inherits it;
/// the external test package and a non-main package do not get it; and a cgo
/// package's `P [P.test]` inherits `runtime/cgo` and `syscall` the same way.
#[test]
fn main_packages_list_the_linkers_implicit_deps() {
    let tmp = std::env::temp_dir().join(format!(
        "guff-golist-linkerdeps-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    write(&tmp.join("go.mod"), "module example.com/m\n\ngo 1.22\n");
    // main, no imports.
    write(&tmp.join("m0/main.go"), "package main\n\nfunc main() {}\n");
    // main + an internal test file that imports nothing either.
    write(&tmp.join("m1/main.go"), "package main\n\nfunc main() {}\n");
    write(&tmp.join("m1/h_test.go"), "package main\n\nfunc helper() {}\n");
    // main + an external test package that imports nothing.
    write(&tmp.join("m2/main.go"), "package main\n\nfunc main() {}\n");
    write(&tmp.join("m2/x_test.go"), "package main_test\n\nfunc helper() {}\n");
    // Only `package main_test` files: go/build names the package `main`.
    write(
        &tmp.join("xo/x_test.go"),
        "package main_test\n\nimport \"testing\"\n\nfunc TestY(t *testing.T) {}\n",
    );
    // main that already reaches runtime: nothing doubles up.
    write(
        &tmp.join("m3/main.go"),
        "package main\n\nimport \"errors\"\n\nvar _ = errors.New\n\nfunc main() {}\n",
    );
    // Not a command.
    write(&tmp.join("lib/a.go"), "package lib\n\nfunc F() {}\n");
    write(&tmp.join("lib/h_test.go"), "package lib\n\nfunc helper() {}\n");

    let cgo = guff_build::Context::default().cgo_enabled;
    if cgo {
        write(
            &tmp.join("c1/a.go"),
            "package c1\n\n// int x;\nimport \"C\"\n\nfunc F() {}\n",
        );
        write(&tmp.join("c1/h_test.go"), "package c1\n\nfunc helper() {}\n");
    }

    let m = |s: &str| format!("example.com/m/{s}");
    let ptest = |s: &str| format!("{} [{}.test]", m(s), m(s));
    let xtest = |s: &str| format!("{}_test [{}.test]", m(s), m(s));

    for (tests, need_deps) in [(true, true), (true, false), (false, true), (false, false)] {
        let cfg = ListConfig {
            dir: tmp.clone(),
            tests,
            need_deps,
            gomodcache: Some(tmp.join("empty-cache")),
            ..ListConfig::default()
        };
        let resp = list_packages(&cfg, &["./...".to_string()])
            .unwrap_or_else(|e| panic!("list tests={tests} need_deps={need_deps}: {e}"));
        let pkg = |id: &str| {
            resp.packages
                .iter()
                .find(|p| p.id == id)
                .unwrap_or_else(|| panic!("{id} listed (tests={tests})"))
        };
        let ctx = format!("tests={tests} need_deps={need_deps}");

        for id in [m("m0"), m("m1"), m("m2"), m("m3")] {
            let p = pkg(&id);
            assert!(p.deps.iter().any(|d| d == "runtime"), "{id} Deps has runtime ({ctx})");
            assert!(
                !p.imports.iter().any(|(src, _)| src == "runtime"),
                "{id} Imports does not ({ctx})"
            );
            let mut sorted = p.deps.clone();
            sorted.dedup();
            assert_eq!(sorted.len(), p.deps.len(), "{id} Deps has no duplicates ({ctx})");
            if need_deps {
                // The runtime closure is listed too, not just named.
                assert!(
                    p.deps.iter().any(|d| d == "internal/abi") && pkg("runtime").standard,
                    "{id} Deps is runtime's closure ({ctx})"
                );
            }
        }
        // `go list` gives m0's Deps exactly runtime's closure plus runtime.
        let rt = pkg("runtime");
        let mut want: Vec<String> = rt.deps.clone();
        want.push("runtime".into());
        want.sort();
        assert_eq!(pkg(&m("m0")).deps, want, "m0 Deps ({ctx})");

        let lib = pkg(&m("lib"));
        assert!(lib.deps.is_empty(), "a non-main package gets nothing ({ctx})");

        if tests {
            assert_eq!(pkg(&ptest("m1")).deps, want, "m1 [m1.test] inherits ({ctx})");
            assert!(pkg(&ptest("m1")).imports.is_empty(), "and lists no Imports ({ctx})");
            assert!(pkg(&xtest("m2")).deps.is_empty(), "main_test is not a command ({ctx})");
            // A command gets `P [P.test]` without internal test files, and
            // testmain links it instead of plain P — when it has files.
            assert_eq!(pkg(&ptest("m2")).deps, want, "m2 [m2.test] exists ({ctx})");
            assert_eq!(pkg(&ptest("m2")).go_files.len(), 1, "with main.go alone ({ctx})");
            let tm_imports = |s: &str| -> Vec<String> {
                let mut v: Vec<String> = pkg(&format!("{}.test", m(s)))
                    .imports
                    .iter()
                    .map(|(src, _)| src.clone())
                    .collect();
                v.sort();
                v
            };
            assert_eq!(
                tm_imports("m2"),
                vec![
                    ptest("m2"),
                    xtest("m2"),
                    "os".to_string(),
                    "reflect".to_string(),
                    "testing".to_string(),
                    "testing/internal/testdeps".to_string(),
                ],
                "m2.test ({ctx})"
            );
            assert_eq!(pkg(&ptest("xo")).deps, want, "xo [xo.test] exists ({ctx})");
            assert!(pkg(&ptest("xo")).go_files.is_empty(), "with no files ({ctx})");
            assert_eq!(pkg(&m("xo")).deps, want, "xo is a command too ({ctx})");
            assert_eq!(
                tm_imports("xo"),
                vec![
                    xtest("xo"),
                    "os".to_string(),
                    "reflect".to_string(),
                    "testing".to_string(),
                    "testing/internal/testdeps".to_string(),
                ],
                "xo.test does not link an empty xo [xo.test] ({ctx})"
            );
            assert!(pkg(&ptest("lib")).deps.is_empty(), "lib [lib.test] gets nothing ({ctx})");
            assert!(
                pkg(&format!("{}.test", m("lib"))).deps.iter().any(|d| d == "runtime"),
                "testmain links runtime ({ctx})"
            );
        } else {
            assert!(!resp.packages.iter().any(|p| p.id.contains(".test")));
        }

        if cgo {
            let implicit = ["runtime", "runtime/cgo", "syscall"];
            let ids = if tests { vec![m("c1"), ptest("c1")] } else { vec![m("c1")] };
            for id in ids {
                let p = pkg(&id);
                for d in implicit {
                    assert!(p.deps.iter().any(|x| x == d), "{id} Deps has {d} ({ctx})");
                }
                assert!(
                    !p.imports.iter().any(|(src, _)| implicit.contains(&src.as_str())),
                    "{id} Imports has none of them ({ctx})"
                );
            }
        }
    }

    let _ = fs::remove_dir_all(&tmp);
}
