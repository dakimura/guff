//! Port of [`github.com/matoous/godox`](https://github.com/matoous/godox)
//! (golangci-lint wrapper in `pkg/golinters/godox`).
//!
//! Defaults match golangci-lint: keywords `TODO`, `BUG`, `FIXME`.
//!
//! DEFERRED: `linters.settings.godox` keyword list wiring.

use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::util::{line_pos, reparse_with_comments};

const DEFAULT_KEYWORDS: &[&str] = &["TODO", "BUG", "FIXME"];

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

fn keyword_match(line: &str, kw: &str) -> bool {
    if line.len() < kw.len() {
        return false;
    }
    if !line[..kw.len()].eq_ignore_ascii_case(kw) {
        return false;
    }
    !has_alphanum_adjacent(&line[kw.len()..])
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "godox requires inspect analyzer".to_string())?;

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
                    for &kw in DEFAULT_KEYWORDS {
                        if !keyword_match(trimmed, kw) {
                            continue;
                        }
                        let display = if trimmed.len() > 40 {
                            format!("{}...", &trimmed[..40])
                        } else {
                            trimmed.to_string()
                        };
                        let joined = DEFAULT_KEYWORDS.join("/");
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
