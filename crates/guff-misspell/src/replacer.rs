//! Spelling correction engine (port of `misspell/replace.go`).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use regex::Regex;

use crate::case::{apply_case, case_style, CaseStyle};
use crate::notwords::remove_not_words;
use crate::options::Options;

static WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-zA-Z0-9']+").unwrap());
static LINE_COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"//[^\n]*").unwrap());
static BLOCK_COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/\*[\s\S]*?\*/").unwrap());

static DICT_MAIN: LazyLock<HashMap<String, String>> =
    LazyLock::new(|| load_dict_tsv(include_str!("../data/dict_main.tsv")));
static DICT_US: LazyLock<HashMap<String, String>> =
    LazyLock::new(|| load_dict_tsv(include_str!("../data/dict_us.tsv")));
static DICT_UK: LazyLock<HashMap<String, String>> =
    LazyLock::new(|| load_dict_tsv(include_str!("../data/dict_uk.tsv")));

/// Default golangci/misspell replacer (DictMain + US locale, no extras/ignores).
/// Shared across packages — building the ~30k-entry map once instead of once
/// per package is the main cold-analyze win for this linter.
static DEFAULT_US: LazyLock<Replacer> = LazyLock::new(|| {
    let mut corrected = DICT_MAIN.clone();
    corrected.extend(DICT_US.iter().map(|(k, v)| (k.clone(), v.clone())));
    Replacer {
        corrected: Arc::new(corrected),
    }
});

/// DictMain only (empty locale, no extras/ignores).
static DEFAULT_MAIN: LazyLock<Replacer> = LazyLock::new(|| Replacer {
    corrected: Arc::new(DICT_MAIN.clone()),
});

/// DictMain + UK locale.
static DEFAULT_UK: LazyLock<Replacer> = LazyLock::new(|| {
    let mut corrected = DICT_MAIN.clone();
    corrected.extend(DICT_UK.iter().map(|(k, v)| (k.clone(), v.clone())));
    Replacer {
        corrected: Arc::new(corrected),
    }
});

/// A single spelling correction in a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    pub line: usize,
    pub column: usize,
    pub original: String,
    pub corrected: String,
}

/// Spelling replacer backed by golangci/misspell dictionaries.
#[derive(Clone)]
pub struct Replacer {
    corrected: Arc<HashMap<String, String>>,
}

impl Replacer {
    /// Default replacer using `DictMain` + US locale (golangci/misspell default).
    pub fn new() -> Self {
        DEFAULT_US.clone()
    }

    /// Build a replacer from golangci-lint `linters.settings.misspell`.
    pub fn from_options(options: &Options) -> Self {
        if options.extra_words.is_empty() && options.ignore_words.is_empty() {
            match options.locale.to_ascii_uppercase().as_str() {
                "US" => return DEFAULT_US.clone(),
                "" => return DEFAULT_MAIN.clone(),
                "UK" | "GB" => return DEFAULT_UK.clone(),
                _ => {}
            }
        }

        let mut corrected = DICT_MAIN.clone();
        match options.locale.to_ascii_uppercase().as_str() {
            "" => {}
            "US" => corrected.extend(DICT_US.iter().map(|(k, v)| (k.clone(), v.clone()))),
            "UK" | "GB" => corrected.extend(DICT_UK.iter().map(|(k, v)| (k.clone(), v.clone()))),
            _ => {}
        }
        for word in &options.extra_words {
            if word.typo.is_empty() || word.correction.is_empty() {
                continue;
            }
            corrected.insert(
                word.typo.to_ascii_lowercase(),
                word.correction.to_ascii_lowercase(),
            );
        }
        for ignore in &options.ignore_words {
            corrected.remove(&ignore.to_ascii_lowercase());
        }
        Self {
            corrected: Arc::new(corrected),
        }
    }

    /// Find misspellings in `input` (default golangci mode: full file as plain text).
    pub fn find_diffs(&self, input: &str) -> Vec<Diff> {
        let mut diffs = Vec::new();
        for (line_idx, line) in input.split_inclusive('\n').enumerate() {
            let line_body = line.strip_suffix('\n').unwrap_or(line);
            diffs.extend(self.diffs_in_line(line_body, line_idx + 1));
        }
        diffs
    }

    /// Find misspellings in comments only (`mode: restricted`).
    pub fn find_diffs_in_comments(&self, input: &str) -> Vec<Diff> {
        let mut diffs = Vec::new();
        for m in LINE_COMMENT_RE.find_iter(input) {
            diffs.extend(self.diffs_in_region(input, m.start(), m.as_str()));
        }
        for m in BLOCK_COMMENT_RE.find_iter(input) {
            diffs.extend(self.diffs_in_region(input, m.start(), m.as_str()));
        }
        diffs
    }

    fn diffs_in_region(&self, input: &str, start: usize, text: &str) -> Vec<Diff> {
        let (line, base_col) = offset_to_line_col(input, start);
        let mut out = Vec::new();
        for mut diff in self.diffs_in_line(text, line) {
            diff.column += base_col;
            out.push(diff);
        }
        out
    }

    fn diffs_in_line(&self, line: &str, line_num: usize) -> Vec<Diff> {
        let redacted = remove_not_words(line);
        let mut out = Vec::new();
        for m in WORD_RE.find_iter(&redacted) {
            let word = &line[m.start()..m.end()];
            if word.is_empty() {
                continue;
            }
            let style = case_style(word);
            if style == CaseStyle::Unknown {
                continue;
            }
            let lower = word.to_ascii_lowercase();
            let Some(corrected_lower) = self.corrected.get(&lower) else {
                continue;
            };
            let corrected = apply_case(corrected_lower, style);
            if corrected == word {
                continue;
            }
            out.push(Diff {
                line: line_num,
                column: m.start(),
                original: word.to_string(),
                corrected,
            });
        }
        out
    }
}

impl Default for Replacer {
    fn default() -> Self {
        Self::new()
    }
}

fn load_dict_tsv(data: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in data.lines() {
        if let Some((typo, correction)) = line.split_once('\t') {
            map.insert(typo.to_string(), correction.to_string());
        }
    }
    map
}

fn offset_to_line_col(input: &str, offset: usize) -> (usize, usize) {
    let before = &input[..offset.min(input.len())];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rfind('\n')
        .map(|idx| offset - idx - 1)
        .unwrap_or(offset);
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::{ExtraWord, Options};

    #[test]
    fn replace_common_typos() {
        let r = Replacer::new();
        let cases = [
            ("I live in Amercia", "America"),
            ("grill brocoli now", "broccoli"),
            ("There is a zeebra", "zebra"),
            ("ten fiels", "fields"),
            ("Closeing Time", "Closing"),
            ("closeing Time", "closing"),
            (" TOOD: foobar", "TODO"),
            (" preceed ", "precede"),
            ("functionallity", "functionality"),
        ];
        for (input, want_word) in cases {
            let diffs = r.find_diffs(input);
            assert!(
                diffs.iter().any(|d| d.corrected == want_word),
                "input {input:?}: want correction {want_word:?}, got {diffs:?}"
            );
        }
    }

    #[test]
    fn clean_text_has_no_diffs() {
        let r = Replacer::new();
        assert!(r.find_diffs("foo other bar").is_empty());
    }

    #[test]
    fn uk_locale_prefers_british_spelling() {
        let r = Replacer::from_options(&Options {
            locale: "UK".into(),
            ..Options::default()
        });
        let diffs = r.find_diffs("favorite color");
        assert!(diffs.iter().any(|d| d.corrected == "favourite"));
        assert!(diffs.iter().any(|d| d.corrected == "colour"));
    }

    #[test]
    fn ignore_words_skip_corrections() {
        let r = Replacer::from_options(&Options {
            locale: "US".into(),
            ignore_words: vec!["amercia".into()],
            ..Options::default()
        });
        assert!(r.find_diffs("Amercia").is_empty());
    }

    #[test]
    fn extra_words_add_corrections() {
        let r = Replacer::from_options(&Options {
            extra_words: vec![ExtraWord {
                typo: "iff".into(),
                correction: "if".into(),
            }],
            ..Options::default()
        });
        let diffs = r.find_diffs("iff x");
        assert!(diffs.iter().any(|d| d.corrected == "if"));
    }

    #[test]
    fn restricted_mode_checks_comments_only() {
        let r = Replacer::new();
        let input = "package p\n// grill brocoli now\nvar x = \"Amercia\"\n";
        let diffs = r.find_diffs_in_comments(input);
        assert!(diffs.iter().any(|d| d.corrected == "broccoli"));
        assert!(!diffs.iter().any(|d| d.original == "Amercia"));
    }

    #[test]
    fn default_us_reuses_shared_map() {
        let a = Replacer::new();
        let b = Replacer::from_options(&Options {
            locale: "US".into(),
            ..Options::default()
        });
        assert!(Arc::ptr_eq(&a.corrected, &b.corrected));
    }

    #[test]
    fn empty_locale_reuses_main_only_map() {
        let a = Replacer::from_options(&Options::default());
        let b = Replacer::from_options(&Options {
            locale: String::new(),
            ..Options::default()
        });
        assert!(Arc::ptr_eq(&a.corrected, &b.corrected));
        assert!(!Arc::ptr_eq(
            &a.corrected,
            &Replacer::new().corrected
        ));
    }
}
