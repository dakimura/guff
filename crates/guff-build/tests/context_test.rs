//! Tests for [`guff_build::Context`].

use guff_build::{default_context, goroot_release, release_tags_for_version, Context, DEFAULT};
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
    // The toolchain's own release when its GOROOT says, as `go list` would.
    let n = goroot_release(&ctx.goroot).unwrap_or(VERSION);
    let want = format!("go1.{n}");
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

#[test]
fn goroot_release_reads_the_version_file() {
    let base = std::env::temp_dir().join(format!("guff-goroot-release-{}", std::process::id()));
    let cases: &[(&str, &str, Option<u32>)] = &[
        ("rel", "go1.27.1\ntime 2026-08-28T16:20:06Z\n", Some(27)),
        ("minor", "go1.26\n", Some(26)),
        ("rc", "go1.28rc1\n", Some(28)),
        ("devel", "devel go1.28-abcdef Tue\n", None),
        ("garbage", "\n", None),
    ];
    for (name, text, want) in cases {
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("VERSION"), text).unwrap();
        assert_eq!(goroot_release(dir.to_str().unwrap()), *want, "{name}");
    }
    assert_eq!(goroot_release(base.join("missing").to_str().unwrap()), None);
    assert_eq!(goroot_release(""), None);
}
