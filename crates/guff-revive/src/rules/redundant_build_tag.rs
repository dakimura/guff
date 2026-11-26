//! `redundant-build-tag` (revive v1.17.0) — within one comment group, a
//! `// +build` line after a `//go:build` line (gofmt has written the latter
//! since Go 1.17), or — from Go 1.21, where go.mod's version is a hard
//! requirement — a `//go:build go1.X` the module's Go version already
//! satisfies. At most one finding per file.

use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{go_lang_version, go_version_at_least, scan_comment_groups};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for i in 0..pass.files().len() {
        let Some(groups) = scan_comment_groups(pass, i) else { continue };
        if let Some(f) = check_file(pass, &groups) {
            failures.push(f);
        }
    }
    failures
}

fn check_file(pass: &Pass<'_>, groups: &[Vec<crate::util::ScannedComment>]) -> Option<Failure> {
    for group in groups {
        let mut has_go_build = false;
        for comment in group {
            if let Some(ver) = comment.text.strip_prefix("//go:build ") {
                has_go_build = true;
                if go_version_at_least(pass, 1, 21) {
                    if let (Some((maj, min)), Some(tag)) = (go_lang_version(pass), parse_go_lang(ver)) {
                        if (maj, min) >= tag {
                            return Some(Failure {
                                rule: "redundant-build-tag",
                                pos: comment.pos,
                                message: format!(
                                    "The build tag {:?} is redundant for Go {maj}.{min} and can be removed",
                                    comment.text
                                ),
                                ..Failure::default()
                            });
                        }
                    }
                }
                continue;
            }
            if has_go_build && comment.text.starts_with("// +build") {
                return Some(Failure {
                    rule: "redundant-build-tag",
                    pos: comment.pos,
                    message: "The build tag \"// +build\" is redundant since Go 1.17 and can be removed"
                        .into(),
                    ..Failure::default()
                });
            }
        }
    }
    None
}

/// `version.IsValid(ver)` for the `//go:build` operand, as `(major, minor)`:
/// a whole-expression Go version such as `go1.21`, `go1.21.3` or `go1.21rc1`
/// — anything else (`linux`, `go1.21 && linux`) is not one.
fn parse_go_lang(ver: &str) -> Option<(u32, u32)> {
    let rest = ver.strip_prefix("go1")?;
    if rest.is_empty() {
        return Some((1, 0));
    }
    let rest = rest.strip_prefix('.')?;
    let minor_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let minor: u32 = rest[..minor_end].parse().ok()?;
    let tail = &rest[minor_end..];
    let valid_tail = tail.is_empty()
        || tail.strip_prefix('.').is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        || ["rc", "beta"].iter().any(|k| {
            tail.strip_prefix(k).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        });
    valid_tail.then_some((1, minor))
}
