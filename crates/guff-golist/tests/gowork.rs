//! go.work multi-module workspace support.

use std::fs;
use std::path::PathBuf;

use guff_golist::{list_packages, ListConfig};

fn write(path: &PathBuf, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

#[test]
fn lists_across_gowork_modules() {
    let tmp = std::env::temp_dir().join(format!(
        "guff-golist-work-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    let app = tmp.join("app");
    let lib = tmp.join("lib");
    write(
        &tmp.join("go.work"),
        "go 1.22\n\nuse (\n\t./app\n\t./lib\n)\n",
    );
    write(
        &app.join("go.mod"),
        "module example.com/app\n\ngo 1.22\n\nrequire example.com/lib v0.0.0\n",
    );
    write(
        &app.join("main.go"),
        "package main\n\nimport \"example.com/lib\"\n\nfunc main() { _ = lib.V }\n",
    );
    write(&lib.join("go.mod"), "module example.com/lib\n\ngo 1.22\n");
    write(&lib.join("lib.go"), "package lib\n\nconst V = 1\n");

    let cfg = ListConfig {
        dir: app.clone(),
        need_deps: true,
        gomodcache: Some(tmp.join("empty-cache")),
        ..ListConfig::default()
    };
    let resp = list_packages(&cfg, &[".".to_string()]).expect("list under go.work");
    assert!(
        resp.packages.iter().any(|p| p.id == "example.com/lib"),
        "expected workspace module example.com/lib; pkgs={:?}",
        resp.packages.iter().map(|p| &p.id).collect::<Vec<_>>()
    );
    assert!(resp.packages.iter().any(|p| p.id == "example.com/app"));

    let _ = fs::remove_dir_all(&tmp);
}
