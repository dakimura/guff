//! Unit tests for `go.mod` parsing and module path mapping.

use std::path::Path;

use guff_build::{find_module_root, module_import_dir, parse_mod_contents};

#[test]
fn module_import_dir_maps_paths() {
    let root = Path::new("/tmp/mod");
    assert_eq!(
        module_import_dir(root, "example.com/mod", "example.com/mod"),
        Some(root.to_path_buf())
    );
    assert_eq!(
        module_import_dir(root, "example.com/mod", "example.com/mod/pkg/sub"),
        Some(root.join("pkg/sub"))
    );
    assert_eq!(
        module_import_dir(root, "example.com/mod", "other.com/pkg"),
        None
    );
}

#[test]
fn find_module_root_walks_upward() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/module/pkg/sub");
    let root = find_module_root(&base).unwrap();
    assert!(root.ends_with("testdata/module"));
}

#[test]
fn parse_mod_contents_minimal() {
    let m = parse_mod_contents("module example.com/x\n\ngo 1.22\n").unwrap();
    assert_eq!(m.module_path, "example.com/x");
    assert_eq!(m.go_version.as_deref(), Some("1.22"));
}
