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

#[test]
fn parse_backquoted_import_paths() {
    // Regression: go/build's `importReader.readString` accepts a raw string
    // literal as well as an interpreted one, and asm2asm-generated files use
    // the backquoted form (bytedance/sonic `internal/native/avx2/*_subr.go`).
    // guff read neither the path nor a byte past it, so `skip_import_spec`
    // returned the same slice and the scan spun forever: cosmos-sdk never
    // finished (golangci-lint: 70.7s for the whole repo).
    let src = br#"package m

import (
	`example.com/m/a`
	"fmt"
	e `errors`
	. `strings`
	_ `embed`
)
"#;
    let info = parse_go_file_info(src).unwrap();
    assert_eq!(
        info.imports,
        vec!["example.com/m/a", "fmt", "errors", "strings", "embed"]
    );
    assert!(!info.imports_c);
}

#[test]
fn parse_backquoted_single_import() {
    let src = b"package single\n\nimport `fmt`\n";
    let info = parse_go_file_info(src).unwrap();
    assert_eq!(info.imports, vec!["fmt"]);
}

#[test]
fn parse_backquoted_named_single_import() {
    let src = b"package alias\n\nimport f `fmt`\n";
    let info = parse_go_file_info(src).unwrap();
    assert_eq!(info.imports, vec!["fmt"]);
}

#[test]
fn parse_backquoted_cgo_import() {
    // `go list -f '{{.CgoFiles}}'` puts this file in CgoFiles, so the raw
    // string form has to set `imports_c` too.
    let src = b"package cgo\n\nimport `C`\n";
    let info = parse_go_file_info(src).unwrap();
    assert_eq!(info.imports, vec!["C"]);
    assert!(info.imports_c);
}

#[test]
fn unparsable_import_spec_terminates_the_scan() {
    // Defence in depth for the shape above: any spec that `skip_import_spec`
    // cannot advance past must end the scan, never repeat.
    let src = b"package m\n\nimport (\n\t$$$\n\t\"fmt\"\n)\n";
    let info = parse_go_file_info(src).unwrap();
    assert_eq!(info.package_name, "m");
    assert!(info.imports.is_empty());
}
