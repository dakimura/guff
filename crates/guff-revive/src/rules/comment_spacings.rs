//! `comment-spacings` — require a space after `//` in comments.

use guff_analysis::Pass;

use crate::failure::Failure;

const DEFAULT_ALLOW: &[&str] = &["//#nosec"];

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for group in &file.comments {
            for comment in &group.list {
                let text = comment.text.as_str();
                if text.len() < 3 {
                    continue;
                }
                if text.starts_with("/*") {
                    if text.len() >= 4 && text.as_bytes()[2] == b'\n' {
                        continue;
                    }
                } else if text.as_bytes()[2] == b' ' || text.as_bytes()[2] == b'\t' {
                    continue;
                }
                if DEFAULT_ALLOW.iter().any(|prefix| text.starts_with(prefix)) {
                    continue;
                }
                failures.push(Failure {
                    rule: "comment-spacings",
                    pos: comment.slash.0 as u32,
                    message: "no space between comment delimiter and comment text".into(),
                });
            }
        }
    }
    failures
}
