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
    assert_eq!(info.imports, vec!["C"]);
}

#[test]
fn parse_package_without_cgo() {
    let src = b"package foo\n\nimport \"fmt\"\n";
    let info = parse_go_file_info(src).unwrap();
    assert_eq!(info.package_name, "foo");
    assert!(!info.imports_c);
    assert_eq!(info.imports, vec!["fmt"]);
}

#[test]
fn parse_named_and_block_imports() {
    let src = br#"package foo

import (
	f "fmt"
	. "strings"
	"os"
)
"#;
    let info = parse_go_file_info(src).unwrap();
    assert_eq!(info.imports, vec!["fmt", "strings", "os"]);
}

#[test]
fn parse_import_alias_starting_with_import() {
    // Regression: `importCmd` must not be mistaken for the `import` keyword
    // (guff-build skip_import_spec; cli pkg/cmd/alias OOM).
    let src = br#"package alias

import (
	importCmd "github.com/cli/cli/v2/pkg/cmd/alias/imports"
	"fmt"
)
"#;
    let info = parse_go_file_info(src).unwrap();
    assert_eq!(
        info.imports,
        vec![
            "github.com/cli/cli/v2/pkg/cmd/alias/imports",
            "fmt"
        ]
    );
}
