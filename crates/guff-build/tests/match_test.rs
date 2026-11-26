//! Tests for build-tag file matching.

use guff_build::{Context, MatchError};

fn ctx(goos: &str, goarch: &str) -> Context {
    Context {
        goos: goos.to_string(),
        goarch: goarch.to_string(),
        compiler: "gc".to_string(),
        release_tags: guff_build::release_tags_for_version(guff_goversion::VERSION),
        ..Context::default()
    }
}

#[test]
fn go_build_linux_matches_linux_not_darwin() {
    let content = b"//go:build linux\n\npackage p\n";
    assert!(ctx("linux", "amd64").match_file("foo.go", content).unwrap());
    assert!(!ctx("darwin", "amd64").match_file("foo.go", content).unwrap());
}

#[test]
fn go_build_darwin_matches_darwin_not_linux() {
    let content = b"//go:build darwin\n\npackage p\n";
    assert!(ctx("darwin", "amd64").match_file("foo.go", content).unwrap());
    assert!(!ctx("linux", "amd64").match_file("foo.go", content).unwrap());
}

#[test]
fn legacy_plus_build_linux() {
    let content = b"// +build linux\n\npackage p\n";
    assert!(ctx("linux", "amd64").match_file("foo.go", content).unwrap());
    assert!(!ctx("darwin", "amd64").match_file("foo.go", content).unwrap());
}

#[test]
fn legacy_plus_build_or_expression() {
    let content = b"// +build linux darwin\n\npackage p\n";
    assert!(ctx("linux", "amd64").match_file("foo.go", content).unwrap());
    assert!(ctx("darwin", "amd64").match_file("foo.go", content).unwrap());
    assert!(!ctx("windows", "amd64").match_file("foo.go", content).unwrap());
}

#[test]
fn go_build_takes_precedence_over_plus_build() {
    let content = b"//go:build linux\n// +build darwin\n\npackage p\n";
    assert!(ctx("linux", "amd64").match_file("foo.go", content).unwrap());
    assert!(!ctx("darwin", "amd64").match_file("foo.go", content).unwrap());
}

#[test]
fn no_constraint_matches_all_platforms() {
    let content = b"package p\n";
    assert!(ctx("linux", "amd64").match_file("foo.go", content).unwrap());
    assert!(ctx("darwin", "amd64").match_file("foo.go", content).unwrap());
}

#[test]
fn filename_os_suffix_linux() {
    let content = b"package p\n";
    assert!(ctx("linux", "amd64").match_file("foo_linux.go", content).unwrap());
    assert!(!ctx("darwin", "amd64").match_file("foo_linux.go", content).unwrap());
}

#[test]
fn filename_os_arch_suffix() {
    let content = b"package p\n";
    assert!(ctx("linux", "amd64").match_file("foo_linux_amd64.go", content).unwrap());
    assert!(!ctx("linux", "arm64").match_file("foo_linux_amd64.go", content).unwrap());
    assert!(!ctx("darwin", "amd64").match_file("foo_linux_amd64.go", content).unwrap());
}

#[test]
fn plain_os_name_without_prefix_is_not_auto_tagged() {
    let content = b"package p\n";
    assert!(ctx("linux", "amd64").match_file("linux.go", content).unwrap());
    assert!(ctx("darwin", "amd64").match_file("linux.go", content).unwrap());
}

#[test]
fn ignored_underscore_prefixed_files() {
    let content = b"package p\n";
    assert!(!ctx("linux", "amd64").match_file("_foo.go", content).unwrap());
}

#[test]
fn custom_build_tag_in_context() {
    let content = b"//go:build integration\n\npackage p\n";
    let with_tag = ctx("linux", "amd64").with_build_tags(["integration"]);
    let without_tag = ctx("linux", "amd64");
    assert!(with_tag.match_file("foo.go", content).unwrap());
    assert!(!without_tag.match_file("foo.go", content).unwrap());
}

#[test]
fn multiple_go_build_is_error() {
    let content = b"//go:build linux\n//go:build darwin\n\npackage p\n";
    let err = ctx("linux", "amd64")
        .match_file("foo.go", content)
        .unwrap_err();
    assert_eq!(err, MatchError::MultipleGoBuild);
}

#[test]
fn use_all_files_overrides_constraints() {
    let content = b"//go:build linux\n\npackage p\n";
    let mut ctxt = ctx("darwin", "amd64");
    ctxt.use_all_files = true;
    assert!(ctxt.match_file("foo.go", content).unwrap());
}

#[test]
fn match_tag_records_consulted_tags() {
    let mut tags = Some(std::collections::HashSet::new());
    let ctxt = ctx("linux", "amd64");
    assert!(ctxt.match_tag("linux", &mut tags));
    let tags = tags.unwrap();
    assert!(tags.contains("linux"));
}
