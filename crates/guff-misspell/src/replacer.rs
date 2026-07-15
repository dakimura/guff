//! Spelling correction engine (port of `misspell/replace.go`).

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::case::{apply_case, case_style, CaseStyle};
use crate::notwords::remove_not_words;

static WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-zA-Z0-9']+").unwrap());

/// A single spelling correction in a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    pub line: usize,
    pub column: usize,
    pub original: String,
    pub corrected: String,
}

/// Spelling replacer backed by golangci/misspell dictionaries.
pub struct Replacer {
    corrected: HashMap<String, String>,
}

impl Replacer {
    /// Default replacer using `DictMain` (golangci/misspell default).
    pub fn new() -> Self {
        static MAIN: LazyLock<HashMap<String, String>> = LazyLock::new(load_main_dict);
        Self {
            corrected: MAIN.clone(),
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

fn load_dict_tsv(data: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in data.lines() {
        if let Some((typo, correction)) = line.split_once('\t') {
            map.insert(typo.to_string(), correction.to_string());
        }
    }
    map
}

fn load_main_dict() -> HashMap<String, String> {
    let mut map = load_dict_tsv(include_str!("../data/dict_main.tsv"));
    for (k, v) in load_dict_tsv(include_str!("../data/dict_us.tsv")) {
        map.insert(k, v);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
