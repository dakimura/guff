//! Port of [`github.com/tetafro/godot`](https://github.com/tetafro/godot)
//! (golangci-lint wrapper in `pkg/golinters/godot`).
//!
//! Defaults match golangci-lint: `scope=declarations`, `period=true`,
//! `capital=false`, empty exclude list.
//!
//! Comments are re-parsed with [`PARSE_COMMENTS`] because production package
//! load uses `Mode::NONE`, which drops lead comments after the package clause.
//!
//! Settings: `linters.settings.godot` (`scope` / `exclude` / `period` / `capital`).
//! DEFERRED: SuggestedFix; block comments inside `const (` / `var (` groups;
//! full `toplevel` / `noinline` scopes (unknown → `declarations`).

use std::sync::OnceLock;

use guff::ast::CommentGroup;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use regex::Regex;

use crate::options::GodotOptions;
use crate::util::{
    block_comments, comment_group_raw_text, declaration_docs, line_pos,
    reparse_with_comments,
};

const NO_PERIOD: &str = "Comment should end in a period";
const NO_CAPITAL: &str = "Sentence should start with a capital letter";

const LAST_CHARS: &[&str] = &[
    ".", "?", "!", ".)", "?)", "!)", "。", "？", "！", "。）", "？）", "！）",
    "<godotSpecialReplacer>",
];

const ABBREVIATIONS: &[&str] = &[
    "i.e.", "i. e.", "e.g.", "e. g.", "etc.", "I.e.", "I. e.", "E.g.", "E. g.",
    "Etc.", "I.E.", "I. E.", "E.G.", "E. G.", "ETC.",
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

fn compile_excludes(patterns: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect()
}

fn match_any(s: &str, excludes: &[Regex]) -> bool {
    excludes.iter().any(|re| re.is_match(s))
}

/// Build checkable comment text (special / excluded lines → replacer).
fn comment_check_text(cg: &CommentGroup, excludes: &[Regex]) -> String {
    if cg.list.is_empty() {
        return String::new();
    }
    if is_special_block(&cg.list[0].text) {
        return String::new();
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
        // A `//` comment is exactly one line, and `"".lines()` yields **zero**
        // items — so splitting dropped a bare `//` entirely and every line
        // after it in the group shifted up by one. Upstream joins one entry per
        // comment, blank included. The blank lines are not decoration here:
        // `check_period` reports the last non-empty line by index, and
        // `check_capital`'s state machine treats a newline as a sentence
        // boundary.
        let lines: Vec<&str> = if is_block {
            stripped.lines().collect()
        } else {
            vec![stripped.as_str()]
        };
        for line in lines {
            let check_line = if is_block {
                line.to_string()
            } else {
                format!("//{line}")
            };
            if is_special_line(&check_line) || match_any(line, excludes) {
                text_lines.push("<godotSpecialReplacer>".to_string());
            } else {
                text_lines.push(line.to_string());
            }
        }
    }
    text_lines.join("\n")
}

/// The **last non-empty line** of the comment, as a 1-based index into `text`,
/// when that line does not end a sentence.
///
/// Upstream reports there rather than at the comment's start, which for a
/// multi-line doc comment is a different line — `checkPeriod` walks the lines
/// backwards and builds its position from the one it stops on. guff reported
/// the comment's first line, so every multi-line comment godot flagged was
/// misplaced: 11 on cobra alone, counted once as a guff-only finding and again
/// as a golangci-only one.
fn check_period(text: &str) -> Option<usize> {
    if text.is_empty() {
        return None;
    }
    let has_letters = text.chars().any(|c| c.is_alphabetic());
    if !has_letters {
        return None;
    }

    let lines: Vec<&str> = text.lines().collect();
    let mut found: Option<(usize, &str)> = None;
    for (i, line) in lines.iter().enumerate().rev() {
        let trimmed = line.trim_end();
        if !trimmed.is_empty() {
            found = Some((i + 1, trimmed));
            break;
        }
    }
    let (idx, last) = found?;
    (!has_suffix(last, LAST_CHARS)).then_some(idx)
}

// godot also computes a *column* — one past the end of the offending line,
// shifted back out to the raw source by
// `pos.column += strings.Index(c.lines[…], …)` — and golangci-lint throws it
// away: its JSON reports column 1 for every godot finding, checked against
// cobra. Only the line survives, so only the line is computed here. `godox`
// carries the same note for the same reason, one function up.

/// Returns true if any sentence after the first should start with a capital
/// and currently starts with lowercase (declaration docs skip the first word).
fn check_capital(text: &str, is_decl: bool) -> bool {
    if text.is_empty() {
        return false;
    }
    let mut cleaned = text.to_string();
    for abbr in ABBREVIATIONS {
        let repl = abbr.replace('.', "_");
        cleaned = cleaned.replace(abbr, &repl);
    }

    // empty=1, endChar=2, endOfSentence=3
    let mut state = if is_decl { 1 } else { 3 };
    for r in cleaned.chars() {
        match r {
            '\n' => {
                if state == 2 {
                    state = 3;
                }
            }
            '.' | '!' | '?' => state = 2,
            ')' if state == 2 => {}
            ' ' => {
                if state == 2 {
                    state = 3;
                }
            }
            c if state == 3 && c.is_lowercase() => return true,
            _ => state = 1,
        }
    }
    false
}

fn collect_comment_groups<'a>(
    fset: &guff::FileSet,
    parsed: &'a guff::ast::File,
    scope: &str,
) -> Vec<&'a CommentGroup> {
    match scope {
        "all" => parsed.comments.iter().collect(),
        // godot's `declarations` scope is `getBlockComments() ++
        // getDeclarationComments()` — the docs of top-level decls *and* the
        // comments inside `var (` / `const (` groups.
        //
        // DEFERRED: toplevel / noinline — fall back to declarations.
        _ => {
            let mut out = block_comments(fset, parsed);
            out.extend(declaration_docs(parsed));
            out
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "godot requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<GodotOptions>("godot")
        .cloned()
        .unwrap_or_default();
    let excludes = compile_excludes(&options.exclude);
    let is_decl_scope = options.scope != "all";

    let mut pending = Vec::new();
    let paths: Vec<_> = pass.pkg().compiled_go_files.clone();
    let fset = pass.fset().clone();
    let n = pass.files().len();

    for i in 0..n {
        let file = &pass.files()[i];
        let Some(path) = paths.get(i) else {
            continue;
        };
        let Some((re_fset, parsed)) = reparse_with_comments(path, pass.pkg().source_bytes(i))
        else {
            continue;
        };
        for doc in collect_comment_groups(&re_fset, &parsed, &options.scope) {
            if comment_group_raw_text(doc).trim().is_empty() {
                continue;
            }
            let text = comment_check_text(doc, &excludes);
            let start_line = re_fset.position(doc.pos()).line;
            let period_at = if options.period {
                check_period(&text)
            } else {
                None
            };
            // DEFERRED: `checkCapital` returns a position per sentence too;
            // `capital` is off by default, so this still reports it at the
            // comment's start.
            let capital = options.capital && check_capital(&text, is_decl_scope);
            if period_at.is_none() && !capital {
                continue;
            }
            if let Some(idx) = period_at {
                let line = start_line + idx as i64 - 1;
                if let Some(pos) = line_pos(&fset, file.pos(), line) {
                    pending.push((pos, NO_PERIOD.to_string()));
                }
            }
            if capital {
                if let Some(pos) = line_pos(&fset, file.pos(), start_line) {
                    pending.push((pos, NO_CAPITAL.to_string()));
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
        name: "godot",
        doc: "Check if comments end in a period",
        url: "https://github.com/tetafro/godot",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
