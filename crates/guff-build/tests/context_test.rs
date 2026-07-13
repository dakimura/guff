//! Tests for [`guff_build::Context`].

use guff_build::{default_context, release_tags_for_version, Context, DEFAULT};
use guff_goversion::VERSION;

#[test]
fn default_context_does_not_panic() {
    let ctx = Context::default();
    assert!(!ctx.goos.is_empty(), "GOOS should be set");
    assert!(!ctx.goarch.is_empty(), "GOARCH should be set");
    assert_eq!(ctx.compiler, "gc");

    // Plan explicitly calls out linux/darwin — both should be recognized hosts.
    let host = std::env::consts::OS;
    if host == "linux" || host == "macos" {
        let expected_goos = if host == "macos" { "darwin" } else { "linux" };
        assert_eq!(ctx.goos, expected_goos);
    }
}

#[test]
fn default_lazy_static_matches_default_fn() {
    let a = Context::default();
    let b = DEFAULT.clone();
    assert_eq!(a.goos, b.goos);
    assert_eq!(a.goarch, b.goarch);
    assert_eq!(a.release_tags, b.release_tags);
}

#[test]
fn release_tags_through_current_version() {
    let tags = release_tags_for_version(VERSION);
    assert!(!tags.is_empty());
    assert_eq!(tags[0], "go1.1");
    let want = format!("go1.{VERSION}");
    assert_eq!(tags.last().map(String::as_str), Some(want.as_str()));
    assert_eq!(tags.len(), VERSION as usize);
}

#[test]
fn default_context_has_release_tags() {
    let ctx = default_context();
    let want = format!("go1.{VERSION}");
    assert_eq!(ctx.release_tags.last().map(String::as_str), Some(want.as_str()));
    assert!(ctx.build_tags.is_empty(), "Default context has no custom build tags");
}

#[test]
fn with_build_tags_appends_tags() {
    let ctx = Context::default().with_build_tags(["integration", "debug"]);
    assert_eq!(ctx.build_tags, vec!["integration", "debug"]);
}

#[test]
fn build_tags_can_be_mutated() {
    let mut ctx = Context::default();
    ctx.build_tags.push("custom".to_string());
    assert!(ctx.build_tags.contains(&"custom".to_string()));
}
