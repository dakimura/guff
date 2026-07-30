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
