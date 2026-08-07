//! Port of [`github.com/matoous/godox`](https://github.com/matoous/godox)
//! (golangci-lint wrapper in `pkg/golinters/godox`).
//!
//! Defaults match golangci-lint: keywords `TODO`, `BUG`, `FIXME`.
//! Settings: `linters.settings.godox.keywords`.

use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::GodoxOptions;
use crate::util::{line_pos, reparse_with_comments};

fn extract_comment_body(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 {
        return Some(text);
    }
    match bytes[1] {
        b'/' => Some(text[2..].strip_prefix(' ').unwrap_or(&text[2..])),
        b'*' => {
            let body = &text[2..];
            Some(body.strip_suffix("*/").unwrap_or(body))
        }
        _ => Some(text),
    }
}

fn has_alphanum_adjacent(rest: &str) -> bool {
    let Some(ch) = rest.chars().next() else {
        return false;
    };
    match ch {
        ':' | ' ' | '(' => false,
        _ => ch.is_alphanumeric(),
    }
}

/// Upstream compares `[]byte` (`bytes.EqualFold(kw, sComment[0:lkw])`), so the
/// prefix test is over **bytes** and never inspects character boundaries.
/// Slicing the `&str` instead panicked on any comment whose first bytes span a
/// multi-byte character — e.g. caddy's `// If ≠0 then …`, where byte 4 falls
/// inside `≠`. Comparing bytes is both the faithful port and the fix.
///
/// A matched keyword is ASCII, so `kw.len()` *is* a character boundary
/// afterwards and the remainder can go back to `&str`.
fn keyword_match(line: &str, kw: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.len() < kw.len() {
        // Upstream has no such guard and indexes out of range instead; it is
        // saved only by its 4-byte minimum line length, so a keyword longer
        // than 4 bytes can panic it. We decline to match rather than crash.
        return false;
    }
    if !bytes[..kw.len()].eq_ignore_ascii_case(kw.as_bytes()) {
        return false;
    }
    !has_alphanum_adjacent(&line[kw.len()..])
}

/// Upstream renders the comment with `fmt.Sprintf("%.40s...", sComment)` when
/// `len(sComment) > 40`. The two limits are in different units on purpose-by-
/// accident: the *condition* counts bytes, while `%.40s` truncates by **runes**
/// — so a 65-byte / 25-rune line is not shortened at all yet still gains the
/// ellipsis. Verified against golangci-lint 2.12.2.
fn display_comment(trimmed: &str) -> String {
    if trimmed.len() <= 40 {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(40).collect();
    format!("{head}...")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "godox requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<GodoxOptions>("godox")
        .cloned()
        .unwrap_or_default();
    let keywords = options.effective_keywords();
    let joined = keywords.join("/");

    let mut pending = Vec::new();
    let paths: Vec<_> = pass.pkg().compiled_go_files.clone();
    let fset = pass.fset().clone();
    let n = pass.files().len();

    for i in 0..n {
        let file = &pass.files()[i];
        let Some(path) = paths.get(i) else {
            continue;
        };
        let Some((re_fset, parsed)) = reparse_with_comments(path) else {
            continue;
        };
        for cg in &parsed.comments {
            for c in &cg.list {
                let Some(body) = extract_comment_body(&c.text) else {
                    continue;
                };
                let start_line = re_fset.position(c.slash).line;
                for (offset, line) in body.lines().enumerate() {
                    let trimmed = line.trim();
                    if trimmed.len() < 4 {
                        continue;
                    }
                    for kw in &keywords {
                        if !keyword_match(trimmed, kw) {
                            continue;
                        }
                        let display = display_comment(trimmed);
                        let line_no = start_line + offset as i64;
                        if let Some(pos) = line_pos(&fset, file.pos(), line_no) {
                            pending.push((
                                pos,
                                format!("Line contains {joined}: {display:?}"),
                            ));
                        }
                        break;
                    }
                }
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "godox",
        doc: "Detects usage of FIXME, TODO and other keywords inside comments",
        url: "https://github.com/matoous/godox",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// caddy has `// If ≠0 then Items starting from that many elements.` and
    /// `// ⚠️ Template functions…`. Byte 4 falls inside the multi-byte
    /// character in both, which used to panic the whole analyzer worker — and a
    /// panicking worker drops every finding it had, silently. Found by the
    /// Phase 2 `default: all` tier (docs/COMPAT-HARDENING.md).
    #[test]
    fn keyword_match_survives_multibyte_prefix() {
        for line in [
            "If ≠0 then Items starting from that many elements.",
            "⚠️ Template functions/actions can access the environment,",
            "日本語のコメント",
            "≠",
        ] {
            for kw in ["TODO", "BUG", "FIXME"] {
                assert!(!keyword_match(line, kw), "{line:?} must not match {kw}");
            }
        }
    }

    #[test]
    fn keyword_match_is_case_insensitive_and_needs_a_separator() {
        assert!(keyword_match("TODO: fix", "TODO"));
        assert!(keyword_match("todo fix", "TODO"));
        assert!(keyword_match("ToDo(me) fix", "TODO"));
        // An adjacent alphanumeric means it is a different word.
        assert!(!keyword_match("TODOS are fine", "TODO"));
        assert!(!keyword_match("TODO1", "TODO"));
        // Shorter than the keyword: upstream indexes out of range here.
        assert!(!keyword_match("TOD", "TODO"));
    }

    /// golangci-lint 2.12.2 on a 65-byte / 25-rune line emits the whole line
    /// followed by `...`: the guard counts bytes, `%.40s` truncates runes.
    #[test]
    fn display_truncates_by_runes_but_guards_by_bytes() {
        let short = "TODO short one";
        assert_eq!(display_comment(short), short);

        let wide = "TODO ⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️";
        assert_eq!(wide.len(), 65);
        assert_eq!(wide.chars().count(), 25);
        // Over the byte guard, under the rune limit: ellipsis, no truncation.
        assert_eq!(display_comment(wide), format!("{wide}..."));

        let long = "TODO ⚠️⚠️⚠️ this comment is deliberately much longer than forty runes total.";
        let got = display_comment(long);
        assert_eq!(got, "TODO ⚠️⚠️⚠️ this comment is deliberately...");
        // Exactly 40 runes kept, then the ellipsis.
        assert_eq!(got.strip_suffix("...").unwrap().chars().count(), 40);
    }
}
