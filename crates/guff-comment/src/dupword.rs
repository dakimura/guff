//! Port of [`github.com/Abirdcfly/dupword`](https://github.com/Abirdcfly/dupword)
//! (golangci-lint wrapper in `pkg/golinters/dupword`).
//!
//! Defaults match golangci-lint: empty keyword filter (flag any duplicate
//! adjacent word), empty ignore list, `comments-only=false` (comments +
//! string literals).
//!
//! Settings: `linters.settings.dupword` (`keywords` / `ignore` / `comments-only`).
//! **Suggested fixes.** Comments and string literals both carry them. The
//! literal half waited on `strconv`: upstream rewrites a literal by
//! `strconv.Unquote` → rewrite → `strconv.Quote`, guff's Go-exact pair lived in
//! `guff-staticcheck`, and an approximate quoter would have put approximate
//! bytes into somebody's source file. Moving that module to `guff-gostd` is the
//! whole of what unblocked it (COMPAT-HARDENING 続き 74).
//!
//! DEFERRED: cross-line duplicate detection spanning adjacent `//` lines;
//! `skip-raw-strings`.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::BasicLit;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_gostd::strconv;

use crate::options::DupwordOptions;
use crate::util::{reparse_with_comments, reparsed_pos};

fn exclude_word(word: &str, ignore: &HashSet<&str>) -> bool {
    let word = word.strip_suffix(',').unwrap_or(word);
    if ignore.contains(word) {
        return true;
    }
    let Some(ch) = word.chars().next() else {
        return true;
    };
    // Match upstream `excludeWords`: unicode.IsDigit || IsPunct || IsSymbol
    // (ASCII-only checks miss box-drawing `│` etc. in tree comments).
    use unicode_general_category::{get_general_category, GeneralCategory as GC};
    matches!(
        get_general_category(ch),
        GC::DecimalNumber
            | GC::ConnectorPunctuation
            | GC::DashPunctuation
            | GC::OpenPunctuation
            | GC::ClosePunctuation
            | GC::InitialPunctuation
            | GC::FinalPunctuation
            | GC::OtherPunctuation
            | GC::MathSymbol
            | GC::CurrencySymbol
            | GC::ModifierSymbol
            | GC::OtherSymbol
    )
}

/// Upstream `checkOneKey`: the rewritten line, and the duplicated words it
/// found.
///
/// This is a state machine over the raw text rather than a scan of
/// `strings.Fields`, because the rewrite has to put the original spacing back —
/// and because two of its quirks are load-bearing for byte parity:
///
/// * The `i == len(raw)-1` arm is the last of an `else if` chain, so when the
///   final character *starts* a word the first arm claims the iteration and the
///   final word is never written. `"a a b"` becomes `"a "`, not `"a b"`.
/// * A duplicate that `excludeWords` rejects takes neither branch, so it is
///   dropped from the rewrite while not being reported.
///
/// Both are upstream's, and v0.1.8 changes the first — which is why this reads
/// the pinned v0.1.7.
fn check_one_key(
    raw: &str,
    key: Option<&str>,
    ignore: &HashSet<&str>,
) -> Option<(String, String)> {
    match key {
        None => {
            let fields: Vec<&str> = raw.split_whitespace().collect();
            if !fields.windows(2).any(|w| w[0] == w[1]) {
                return None;
            }
        }
        // `strings.Split(raw, key)` has fewer than two parts exactly when the
        // key does not occur.
        Some(k) => {
            if !raw.contains(k) {
                return None;
            }
        }
    }

    let mut found: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut new_line = String::new();
    let (mut word_start, mut space_start) = (0usize, 0usize);
    let (mut cur_word, mut pre_word) = ("", "");
    let mut last_space = "";
    // Go starts with the zero rune, which is not a space.
    let mut last_rune = '\0';
    let mut find = false;
    let n = raw.len();

    for (i, r) in raw.char_indices() {
        if !r.is_whitespace() && last_rune.is_whitespace() {
            let mut symbol = &raw[space_start..i];
            let keyed = key.is_none_or(|k| cur_word == k);
            if keyed && cur_word == pre_word && !cur_word.is_empty() {
                if !exclude_word(cur_word, ignore) {
                    find = true;
                    found.insert(cur_word);
                    new_line.push_str(last_space);
                    symbol = "";
                }
            } else {
                new_line.push_str(last_space);
                new_line.push_str(cur_word);
            }
            last_space = symbol;
            pre_word = cur_word;
            word_start = i;
        } else if r.is_whitespace() && !last_rune.is_whitespace() {
            space_start = i;
            cur_word = &raw[word_start..i];
        } else if i + 1 == n {
            let word = &raw[word_start..];
            let keyed = key.is_none_or(|k| word == k);
            if keyed && word == pre_word {
                if !exclude_word(word, ignore) {
                    find = true;
                    found.insert(word);
                }
            } else {
                new_line.push_str(last_space);
                new_line.push_str(word);
            }
        }
        last_rune = r;
    }

    if !find {
        return None;
    }
    let words = found.into_iter().collect::<Vec<_>>().join(",");
    Some((new_line, words))
}

/// Upstream `Check`: run every configured keyword over the text in turn, each
/// pass seeing the previous one's rewrite.
///
/// With no keywords the reported word list is what `checkOneKey` found; with
/// keywords it is the *keys* that matched, which is upstream's own asymmetry.
fn find_duplicates(
    raw: &str,
    keywords: &[String],
    ignore: &HashSet<&str>,
) -> Option<(String, String)> {
    if keywords.is_empty() {
        return check_one_key(raw, None, ignore);
    }
    let mut current = raw.to_string();
    let mut update = String::new();
    let mut keyword = String::new();
    let mut find = false;
    for key in keywords {
        if let Some((new_line, _)) = check_one_key(&current, Some(key), ignore) {
            current = new_line.clone();
            update = new_line;
            find = true;
            if keyword.is_empty() {
                keyword = key.clone();
            } else {
                keyword.push(',');
                keyword.push_str(key);
            }
        }
    }
    find.then_some((update, keyword))
}

fn is_example_output(comment: &str) -> bool {
    comment.starts_with("// Output:")
        || comment.starts_with("// output:")
        || comment.starts_with("// Unordered output:")
        || comment.starts_with("// unordered output:")
}

fn check_string_lit(
    lit: &BasicLit,
    keywords: &[String],
    ignore: &HashSet<&str>,
    pending: &mut Vec<(u32, String, Option<TextEdit>)>,
) {
    if lit.kind != Some(Token::STRING) {
        return;
    }
    // Upstream (`fixDuplicateWordInString`, dupword.go:185) unquotes first and
    // checks the *unquoted* text, so an escape is compared as the byte it
    // stands for. On failure it falls back to the raw literal — delimiters and
    // all — and then writes the rewrite back **unquoted**, because its `quote`
    // flag is `value != lit.Value` rather than `err == nil`. Both halves matter:
    // checking the raw text finds different duplicates, and re-quoting a
    // fallback would add a second layer of quotes to a malformed literal.
    let value = match strconv::unquote(&lit.value) {
        Ok(v) => v,
        Err(_) => lit.value.clone(),
    };
    let quote = value != lit.value;
    let Some((update, words)) = find_duplicates(&value, keywords, ignore) else {
        return;
    };
    let update = if quote {
        strconv::quote(&update)
    } else {
        update
    };
    // `lit.Pos()` to `lit.End()`: the whole literal, delimiters included.
    let pos = lit.value_pos.0 as u32;
    let end = pos + lit.value.len() as u32;
    pending.push((
        pos,
        format!("Duplicate words ({words}) found"),
        Some(TextEdit {
            pos,
            end,
            new_text: update,
        }),
    ));
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
            if let Some((re_fset, parsed)) =
                reparse_with_comments(path, pass.pkg().source_bytes(i))
            {
                for cg in &parsed.comments {
                    if is_test && !cg.list.is_empty() && is_example_output(&cg.list[0].text) {
                        continue;
                    }
                    for c in &cg.list {
                        if is_example_output(&c.text) {
                            continue;
                        }
                        if let Some((update, words)) =
                            find_duplicates(&c.text, &options.keywords, &ignore)
                        {
                            if let Some(pos) =
                                reparsed_pos(&fset, file.pos(), &re_fset, c.slash)
                            {
                                // The edit spans the comment exactly: upstream
                                // uses `c.Slash` to `c.End()`, and a comment
                                // ends where its own text does.
                                let end = pos + c.text.len() as u32;
                                pending.push((
                                    pos,
                                    format!("Duplicate words ({words}) found"),
                                    Some(TextEdit {
                                        pos,
                                        end,
                                        new_text: update,
                                    }),
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

    for (pos, message, edit) in pending {
        let Some(edit) = edit else {
            pass.reportf(pos, message);
            continue;
        };
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Update".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
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
