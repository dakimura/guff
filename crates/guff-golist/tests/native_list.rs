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
