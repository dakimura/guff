//! `comment-spacings` — require a space after `//` in comments.

use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;
use crate::settings::RuleArgument;
use crate::util::scan_comments;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    // Upstream's allow list comes entirely from the rule's arguments, each
    // prefixed with `//`. There is no built-in list: everything else it lets
    // through is matched by `is_directive_comment`.
    let allow_list: Vec<String> = config::rule_arguments(pass, "comment-spacings")
        .iter()
        .filter_map(|arg| match arg {
            RuleArgument::String(s) => Some(format!("//{s}")),
            _ => None,
        })
        .collect();

    let mut failures = Vec::new();
    for i in 0..pass.files().len() {
        // Upstream walks `file.AST.Comments`, which holds every comment. The
        // analysis AST is parsed without them, so this rule saw an empty list
        // and reported nothing at all in production. Only the comment text is
        // needed, so scan rather than reparse.
        let Some(comments) = scan_comments(pass, i) else {
            continue;
        };
        for comment in &comments {
            let text = comment.text.as_str();
            let bytes = text.as_bytes();
            if bytes.len() < 3 {
                continue;
            }
            // A `/*` comment is fine when it opens with a newline. If it
            // does not, it still has to pass the space/tab check below —
            // upstream applies that to block and line comments alike, so
            // `/* text */` is as acceptable as `// text`.
            if bytes[1] == b'*' && bytes[2] == b'\n' {
                continue;
            }
            if bytes[2] == b' ' || bytes[2] == b'\t' {
                continue;
            }
            if allow_list.iter().any(|prefix| text.starts_with(prefix)) {
                continue;
            }
            if is_directive_comment(text) {
                continue;
            }
            failures.push(Failure {
                rule: "comment-spacings",
                pos: comment.pos,
                message: "no space between comment delimiter and comment text".into(),
                ..Failure::default()
            });
        }
    }
    failures
}

/// Port of upstream's `directiveCommentRE`:
/// `^//(line |extern |export |[a-z0-9]+:[a-z0-9])`.
///
/// Note what this does *not* cover, because guff used to allow them and
/// upstream does not: a bare `//nolint` (no colon), `//sys ` and `//#nosec`.
fn is_directive_comment(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("//") else {
        return false;
    };
    for kw in ["line ", "extern ", "export "] {
        if rest.starts_with(kw) {
            return true;
        }
    }
    // `[a-z0-9]+:[a-z0-9]`
    let bytes = rest.as_bytes();
    let name_len = bytes
        .iter()
        .position(|b| !b.is_ascii_lowercase() && !b.is_ascii_digit())
        .unwrap_or(bytes.len());
    if name_len == 0 {
        return false;
    }
    matches!(bytes.get(name_len), Some(b':'))
        && bytes
            .get(name_len + 1)
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::is_directive_comment;

    #[test]
    fn directive_regex_matches_upstream() {
        for ok in [
            "//go:build linux",
            "//line foo.go:1",
            "//extern name",
            "//export Name",
            "//nolint:errcheck",
            "//x1:y",
        ] {
            assert!(is_directive_comment(ok), "{ok} should be a directive");
        }
        // Upstream's regex rejects these; guff used to allow them.
        for no in [
            "//nolint",
            "//#nosec",
            "//sys Foo()",
            "//go:",
            "//Go:build",
            "//:x",
        ] {
            assert!(!is_directive_comment(no), "{no} should not be a directive");
        }
    }
}
