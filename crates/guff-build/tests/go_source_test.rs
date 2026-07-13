//! Unit tests for Go source header parsing.

use guff_build::go_source::parse_go_file_info;

#[test]
fn parse_package_and_cgo_import() {
    let src = br#"package main

import "C"

func main() {}
"#;
    let info = parse_go_file_info(src).unwrap();
    assert_eq!(info.package_name, "main");
    assert!(info.imports_c);
}

#[test]
fn parse_package_without_cgo() {
    let src = b"package foo\n\nimport \"fmt\"\n";
    let info = parse_go_file_info(src).unwrap();
    assert_eq!(info.package_name, "foo");
    assert!(!info.imports_c);
}
