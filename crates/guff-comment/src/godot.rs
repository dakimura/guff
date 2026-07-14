//! Port of [`github.com/tetafro/godot`](https://github.com/tetafro/godot)
//! (golangci-lint wrapper in `pkg/golinters/godot`).
//!
//! Defaults match golangci-lint: `scope=declarations`, `period=true`,
//! `capital=false`, empty exclude list.
//!
//! Comments are re-parsed with [`PARSE_COMMENTS`] because production package
//! load uses `Mode::NONE`, which drops lead comments after the package clause.
//!
//! DEFERRED: `linters.settings.godot` (scope / exclude / period / capital);
//! SuggestedFix; block comments inside `const (` / `var (` groups;
//! full `toplevel` / `noinline` / `all` scopes.

use std::sync::OnceLock;

use guff::ast::CommentGroup;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::util::{
    comment_group_raw_text, declaration_docs, line_pos, reparse_with_comments,
};

const NO_PERIOD: &str = "Comment should end in a period";

const LAST_CHARS: &[&str] = &[
    ".", "?", "!", ".)", "?)", "!)", "。", "？", "！", "。）", "？）", "！）",
    "<godotSpecialReplacer>",
];

fn has_suffix(s: &str, suffixes: &[&str]) -> bool {
    suffixes.iter().any(|suf| s.ends_with(suf))
}

fn is_special_block(comment: &str) -> bool {
    if comment.starts_with("/*")
        && (comment.contains("#include") || comment.contains("#define"))
    {
        return true;
    }
    comment.starts_with("// Output:") || comment.starts_with("// Unordered output:")
}

fn is_special_line(line: &str) -> bool {
    if line.starts_with("//export ") {
        return true;
    }
    let mut body = line;
    if let Some(rest) = body.strip_prefix("//") {
        body = rest;
    } else if let Some(rest) = body.strip_prefix("/*") {
        body = rest;
    }
    if body.starts_with("  ") || body.starts_with(" \t") || body.starts_with('\t') {
        return true;
    }
    let trimmed = body.trim();
    let tag = trimmed.trim_start_matches('+');
    if let Some((name, _)) = tag.split_once(':') {
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return true;
        }
    }
    if trimmed.starts_with('#') {
        let rest = &trimmed[1..];
        let end = rest
            .find(|c: char| !c.is_ascii_lowercase())
            .unwrap_or(rest.len());
        if end > 0 && (end == rest.len() || rest[end..].starts_with(char::is_whitespace)) {
            return true;
        }
    }
    if trimmed.contains("://") {
        if let Some(idx) = trimmed.rfind("://") {
            let before = &trimmed[..idx];
            if before
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_lowercase())
                .count()
                > 0
                && !trimmed[idx..].contains(char::is_whitespace)
            {
                return true;
            }
        }
    }
    trimmed.starts_with("+build")
}

fn check_period(cg: &CommentGroup) -> bool {
    if cg.list.is_empty() {
        return false;
    }
    if is_special_block(&cg.list[0].text) {
        return false;
    }

    let mut text_lines = Vec::new();
    for c in &cg.list {
        let raw = &c.text;
        let is_block = raw.starts_with("/*");
        let stripped = if is_block {
            let mut s = raw[2..].to_string();
            if s.ends_with("*/") {
                s.truncate(s.len() - 2);
            }
            s
        } else {
            raw.strip_prefix("//").unwrap_or(raw).to_string()
        };
        for line in stripped.lines() {
            let check_line = if is_block {
                line.to_string()
            } else {
                format!("//{line}")
            };
            if is_special_line(&check_line) {
                text_lines.push("<godotSpecialReplacer>".to_string());
            } else {
                text_lines.push(line.to_string());
            }
        }
    }
    let text = text_lines.join("\n");
    if text.is_empty() {
        return false;
    }

    let has_letters = text.chars().any(|c| c.is_alphabetic());
    if !has_letters {
        return false;
    }

    let lines: Vec<&str> = text.lines().collect();
    let mut last_nonempty: Option<&str> = None;
    for line in lines.iter().rev() {
        let trimmed = line.trim_end();
        if !trimmed.is_empty() {
            last_nonempty = Some(trimmed);
            break;
        }
    }
    let Some(last) = last_nonempty else {
        return false;
    };
    !has_suffix(last, LAST_CHARS)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "godot requires inspect analyzer".to_string())?;

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
        for doc in declaration_docs(&parsed) {
            if comment_group_raw_text(doc).trim().is_empty() {
                continue;
            }
            if !check_period(doc) {
                continue;
            }
            let line = re_fset.position(doc.pos()).line;
            if let Some(pos) = line_pos(&fset, file.pos(), line) {
                pending.push((pos, NO_PERIOD.to_string()));
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
        name: "godot",
        doc: "Check if comments end in a period",
        url: "https://github.com/tetafro/godot",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
