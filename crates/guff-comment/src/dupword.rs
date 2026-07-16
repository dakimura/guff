//! Port of [`github.com/Abirdcfly/dupword`](https://github.com/Abirdcfly/dupword)
//! (golangci-lint wrapper in `pkg/golinters/dupword`).
//!
//! Defaults match golangci-lint: empty keyword filter (flag any duplicate
//! adjacent word), empty ignore list, `comments-only=false` (comments +
//! string literals).
//!
//! Settings: `linters.settings.dupword` (`keywords` / `ignore` / `comments-only`).
//! DEFERRED: SuggestedFix; cross-line duplicate detection spanning adjacent
//! `//` lines; `skip-raw-strings`.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::BasicLit;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::DupwordOptions;
use crate::util::{line_pos, reparse_with_comments};

fn exclude_word(word: &str, ignore: &HashSet<&str>) -> bool {
    let word = word.strip_suffix(',').unwrap_or(word);
    if ignore.contains(word) {
        return true;
    }
    let Some(ch) = word.chars().next() else {
        return true;
    };
    ch.is_ascii_digit() || ch.is_ascii_punctuation()
}

/// Detect adjacent duplicate words; return joined list of duplicates if any.
fn find_duplicates(raw: &str, keywords: &[String], ignore: &HashSet<&str>) -> Option<String> {
    if !keywords.is_empty() {
        let mut found = Vec::new();
        for key in keywords {
            if let Some(words) = find_duplicates_one_key(raw, Some(key), ignore) {
                for w in words.split(',') {
                    if !found.contains(&w.to_string()) {
                        found.push(w.to_string());
                    }
                }
            }
        }
        if found.is_empty() {
            return None;
        }
        found.sort_unstable();
        return Some(found.join(","));
    }
    find_duplicates_one_key(raw, None, ignore).map(|s| {
        let mut parts: Vec<&str> = s.split(',').collect();
        parts.sort_unstable();
        parts.join(",")
    })
}

fn find_duplicates_one_key(
    raw: &str,
    key: Option<&str>,
    ignore: &HashSet<&str>,
) -> Option<String> {
    let fields: Vec<&str> = raw.split_whitespace().collect();
    if fields.len() < 2 {
        return None;
    }
    let mut found = Vec::new();
    for w in fields.windows(2) {
        if w[0] != w[1] || w[0].is_empty() {
            continue;
        }
        if let Some(k) = key {
            if w[0] != k {
                continue;
            }
        }
        if exclude_word(w[0], ignore) {
            continue;
        }
        if !found.contains(&w[0]) {
            found.push(w[0]);
        }
    }
    if found.is_empty() {
        return None;
    }
    Some(found.join(","))
}

fn is_example_output(comment: &str) -> bool {
    comment.starts_with("// Output:")
        || comment.starts_with("// output:")
        || comment.starts_with("// Unordered output:")
        || comment.starts_with("// unordered output:")
}

fn unquote_string(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'`' && bytes[value.len() - 1] == b'`')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

fn check_string_lit(
    lit: &BasicLit,
    keywords: &[String],
    ignore: &HashSet<&str>,
    pending: &mut Vec<(u32, String)>,
) {
    if lit.kind != Some(Token::STRING) {
        return;
    }
    let value = unquote_string(&lit.value);
    if let Some(words) = find_duplicates(&value, keywords, ignore) {
        pending.push((
            lit.value_pos.0 as u32,
            format!("Duplicate words ({words}) found"),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "dupword requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<DupwordOptions>("dupword")
        .cloned()
        .unwrap_or_default();
    let ignore: HashSet<&str> = options.ignore.iter().map(String::as_str).collect();

    let mut pending = Vec::new();
    let paths: Vec<_> = pass.pkg().compiled_go_files.clone();
    let fset = pass.fset().clone();
    let n = pass.files().len();

    for i in 0..n {
        let file = &pass.files()[i];
        let path = paths.get(i);
        let is_test = path
            .and_then(|p| p.to_str())
            .map(|s| s.ends_with("_test.go"))
            .unwrap_or(false);

        if let Some(path) = path {
            if let Some((re_fset, parsed)) = reparse_with_comments(path) {
                for cg in &parsed.comments {
                    if is_test && !cg.list.is_empty() && is_example_output(&cg.list[0].text) {
                        continue;
                    }
                    for c in &cg.list {
                        if is_example_output(&c.text) {
                            continue;
                        }
                        if let Some(words) =
                            find_duplicates(&c.text, &options.keywords, &ignore)
                        {
                            let line = re_fset.position(c.slash).line;
                            if let Some(pos) = line_pos(&fset, file.pos(), line) {
                                pending.push((
                                    pos,
                                    format!("Duplicate words ({words}) found"),
                                ));
                            }
                        }
                    }
                }
            }
        }

        if options.comments_only {
            continue;
        }

        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::BasicLit(lit) = n {
                check_string_lit(lit, &options.keywords, &ignore, &mut pending);
            }
            true
        });
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "dupword",
        doc: "checks for duplicate words in the source code (usually miswritten)",
        url: "https://github.com/Abirdcfly/dupword",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
