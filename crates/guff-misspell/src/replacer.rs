//! Spelling correction engine (port of `misspell/replace.go`).

use std::sync::{Arc, LazyLock};

use regex::Regex;
use rustc_hash::FxHashMap;

use crate::case::{apply_case, case_style, CaseStyle};
use crate::notwords::remove_not_words;
use crate::options::Options;

static LINE_COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"//[^\n]*").unwrap());
static BLOCK_COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/\*[\s\S]*?\*/").unwrap());

/// Word characters matching golangci/misspell's `[a-zA-Z0-9']+`.
#[inline]
fn is_word_byte(b: u8) -> bool {
    matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'\'')
}

static DICT_MAIN: LazyLock<Vec<(Box<str>, Box<str>)>> =
    LazyLock::new(|| load_dict_tsv(include_str!("../data/dict_main.tsv")));
static DICT_US: LazyLock<Vec<(Box<str>, Box<str>)>> =
    LazyLock::new(|| load_dict_tsv(include_str!("../data/dict_us.tsv")));
static DICT_UK: LazyLock<Vec<(Box<str>, Box<str>)>> =
    LazyLock::new(|| load_dict_tsv(include_str!("../data/dict_uk.tsv")));

/// Default golangci/misspell replacer (DictMain + US locale, no extras/ignores).
/// Shared across packages — building the ~30k-entry map once instead of once
/// per package is the main cold-analyze win for this linter.
static DEFAULT_US: LazyLock<Replacer> =
    LazyLock::new(|| Replacer::build(DICT_MAIN.iter().chain(DICT_US.iter()), &[]));

/// DictMain only (empty locale, no extras/ignores).
static DEFAULT_MAIN: LazyLock<Replacer> =
    LazyLock::new(|| Replacer::build(DICT_MAIN.iter(), &[]));

/// DictMain + UK locale.
static DEFAULT_UK: LazyLock<Replacer> =
    LazyLock::new(|| Replacer::build(DICT_MAIN.iter().chain(DICT_UK.iter()), &[]));

/// A single spelling correction in a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    pub line: usize,
    pub column: usize,
    pub original: String,
    pub corrected: String,
}

/// One dictionary key.
///
/// A key can appear more than once: `AddRuleList` appends the locale list to
/// `DictMain` without checking for duplicates, and the two halves of upstream
/// then disagree on purpose. `Compile` builds `corrected` with a plain map
/// assignment, so **the last** value wins there, while the trie's `add` keeps
/// the **first** (`if t.priority == 0`). Both are needed: the engine writes
/// `first`, and `recheckLine` compares its output against `last`.
#[derive(Clone, Debug)]
struct Entry {
    /// Position of the first pair with this key in the concatenated rule list.
    /// Upstream's trie gives earlier pairs the higher priority, so this is what
    /// decides which of two overlapping keys wins — *not* their length.
    index: u32,
    /// Value of that first pair: what the engine writes when this key matches.
    first: Box<str>,
    /// Value of the last pair, when it differs from `first`.
    last: Option<Box<str>>,
}

impl Entry {
    /// What `corrected[word]` answers.
    fn corrected(&self) -> &str {
        self.last.as_deref().unwrap_or(&self.first)
    }
}

/// Spelling replacer backed by golangci/misspell dictionaries.
#[derive(Clone)]
pub struct Replacer {
    entries: Arc<FxHashMap<Box<str>, Entry>>,
    /// Longest key, so the prefix scan stops instead of walking the word.
    max_key_len: usize,
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

        // `appendExtraWords` is another `AddRuleList`, so the extra pairs land
        // after the locale list and carry the lowest priority. `RemoveRule`
        // then drops every pair with a matching key, which is why the ignore
        // list is applied to the built map rather than folded into the order.
        let extra: Vec<(Box<str>, Box<str>)> = options
            .extra_words
            .iter()
            .filter(|w| !w.typo.is_empty() && !w.correction.is_empty())
            .map(|w| {
                (
                    w.typo.to_ascii_lowercase().into_boxed_str(),
                    w.correction.to_ascii_lowercase().into_boxed_str(),
                )
            })
            .collect();
        let ignore: Vec<String> = options
            .ignore_words
            .iter()
            .map(|w| w.to_ascii_lowercase())
            .collect();
        let locale = options.locale.to_ascii_uppercase();
        let dict: &[(Box<str>, Box<str>)] = match locale.as_str() {
            "US" => &DICT_US,
            "UK" | "GB" => &DICT_UK,
            _ => &[],
        };
        Self::build(
            DICT_MAIN.iter().chain(dict.iter()).chain(extra.iter()),
            &ignore,
        )
    }

    fn build<'a, I>(pairs: I, ignore: &[String]) -> Self
    where
        I: Iterator<Item = &'a (Box<str>, Box<str>)>,
    {
        let mut entries: FxHashMap<Box<str>, Entry> = FxHashMap::default();
        let mut max_key_len = 0usize;
        for (index, (key, value)) in pairs.enumerate() {
            if ignore.iter().any(|w| w.as_str() == key.as_ref()) {
                continue;
            }
            match entries.get_mut(key.as_ref()) {
                Some(existing) => {
                    existing.last = (value != &existing.first).then(|| value.clone());
                }
                None => {
                    max_key_len = max_key_len.max(key.len());
                    entries.insert(
                        key.clone(),
                        Entry {
                            index: index as u32,
                            first: value.clone(),
                            last: None,
                        },
                    );
                }
            }
        }
        Self {
            entries: Arc::new(entries),
            max_key_len,
        }
    }

    /// `genericReplacer.Replace` over one lowercase ASCII word.
    ///
    /// Left to right, non-overlapping; at each position the winner is the
    /// **highest-priority** key that matches there, which is the one written
    /// earliest in the rule list — not the longest. That is the whole reason
    /// upstream stays quiet on `normalise`: `DictMain`'s `normalis → normals`
    /// is written before `DictAmerican`'s `normalise → normalize`, so the
    /// prefix wins and the word becomes `normalse`.
    fn engine_replace(&self, word: &str) -> String {
        let mut out = String::with_capacity(word.len());
        let mut i = 0;
        while i < word.len() {
            let limit = (word.len() - i).min(self.max_key_len);
            let mut best: Option<(&Entry, usize)> = None;
            for len in 1..=limit {
                if let Some(e) = self.entries.get(&word[i..i + len]) {
                    if best.is_none_or(|(b, _)| e.index < b.index) {
                        best = Some((e, len));
                    }
                }
            }
            match best {
                Some((e, len)) => {
                    out.push_str(&e.first);
                    i += len;
                }
                None => {
                    out.push_str(&word[i..i + 1]);
                    i += 1;
                }
            }
        }
        out
    }

    /// The correction upstream would report for `word` (already lowercase), or
    /// `None`.
    ///
    /// `recheckLine` runs the whole engine over the word and reports only when
    /// the result is the word's own correction:
    ///
    /// ```go
    /// if StringEqualFold(r.corrected[strings.ToLower(word)], newword) { …report… }
    /// // Word got corrected into something unknown. Ignore it
    /// ```
    ///
    /// Looking the word up in a single merged map — which is what guff did —
    /// skips that check, and reports the 97 `locale: US` words (11 for UK) that
    /// an earlier `DictMain` prefix rule swallows.
    fn correction_for(&self, word: &str) -> Option<&str> {
        let entry = self.entries.get(word)?;
        let corrected = entry.corrected();
        self.engine_replace(word)
            .eq_ignore_ascii_case(corrected)
            .then_some(corrected)
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
        // Scan the redacted view so URL/path/email spans are skipped, but
        // report substrings from the original line (same byte ranges).
        let bytes = redacted.as_bytes();
        debug_assert_eq!(bytes.len(), line.len());
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if !is_word_byte(bytes[i]) {
                i += 1;
                continue;
            }
            let start = i;
            i += 1;
            while i < bytes.len() && is_word_byte(bytes[i]) {
                i += 1;
            }
            // Word spans are ASCII `[a-zA-Z0-9']+`; same range is valid UTF-8 in `line`.
            let word = &line[start..i];
            let style = case_style(word);
            if style == CaseStyle::Unknown {
                continue;
            }
            // Dict keys are lowercase; skip the alloc when the word already is.
            let lower_owned;
            let lower: &str = if word.bytes().any(|b| b.is_ascii_uppercase()) {
                lower_owned = word.to_ascii_lowercase();
                &lower_owned
            } else {
                word
            };
            let Some(corrected_lower) = self.correction_for(lower) else {
                continue;
            };
            let corrected = apply_case(corrected_lower, style);
            if corrected == word {
                continue;
            }
            out.push(Diff {
                line: line_num,
                column: start,
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

/// Parse a dictionary **in file order**: the order is the priority, so this
/// cannot be a map.
fn load_dict_tsv(data: &str) -> Vec<(Box<str>, Box<str>)> {
    data.lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(typo, correction)| (typo.into(), correction.into()))
        .collect()
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
    fn an_earlier_prefix_rule_silences_the_locale_word() {
        // golangci-lint reports none of these with `locale: US`, because
        // `DictMain` carries a correction for a *prefix* of each one earlier in
        // the rule list — `capitalis→capitals`, `criticis→critics`,
        // `crystallis→crystals`, `energis→energies`, `normalis→normals` — and
        // the trie takes the earlier rule, not the longer one. The word then
        // corrects into something that is not its own correction, and
        // `recheckLine` drops it.
        //
        // guff merged main and locale into one map and looked the whole word up,
        // so it reported all of them: two turned up on external-dns
        // (`normalise`, `normalises`), and the dictionaries carry 97 such words
        // for US and 11 for UK.
        let r = Replacer::new();
        for word in [
            "capitalise",
            "criticise",
            "crystallise",
            "energise",
            "normalise",
            "normalises",
        ] {
            assert!(
                r.find_diffs(&format!("// {word} here")).is_empty(),
                "{word} must be silent: {:?}",
                r.find_diffs(&format!("// {word} here"))
            );
        }
        // Controls: British spellings with no earlier prefix rule, and a plain
        // `DictMain` typo. All of these upstream does report.
        for (word, want) in [
            ("analyse", "analyze"),
            ("colour", "color"),
            ("behaviour", "behavior"),
            ("organise", "organize"),
            ("recognise", "recognize"),
            ("seperate", "separate"),
        ] {
            let diffs = r.find_diffs(&format!("// {word} here"));
            assert_eq!(diffs.len(), 1, "{word}: {diffs:?}");
            assert_eq!(diffs[0].corrected, want, "{word}");
        }
    }

    #[test]
    fn engine_replace_takes_the_earlier_rule_not_the_longer_one() {
        let r = Replacer::new();
        // `normalis` (DictMain) beats `normalise` (DictAmerican) and leaves the
        // trailing `e` behind — which is not `normalize`, so nothing is
        // reported for the word.
        assert_eq!(r.engine_replace("normalise"), "normalse");
        // A word whose own rule is the earliest match replaces whole.
        assert_eq!(r.engine_replace("analyse"), "analyze");
        // A word with no rule at all is copied through byte by byte.
        assert_eq!(r.engine_replace("endpoint"), "endpoint");
    }

    #[test]
    fn locale_none_reports_neither_the_shadowed_word_nor_the_plain_british_one() {
        // Without a locale there is no `normalise` rule to be shadowed and no
        // `analyse` rule either, so the whole family is silent — the shadowing
        // only bites once a locale list is appended.
        let r = Replacer::from_options(&Options::default());
        assert!(r.find_diffs("// normalise and analyse").is_empty());
        assert!(!r.find_diffs("// seperate").is_empty());
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
        assert!(Arc::ptr_eq(&a.entries, &b.entries));
    }

    #[test]
    fn empty_locale_reuses_main_only_map() {
        let a = Replacer::from_options(&Options::default());
        let b = Replacer::from_options(&Options {
            locale: String::new(),
            ..Options::default()
        });
        assert!(Arc::ptr_eq(&a.entries, &b.entries));
        assert!(!Arc::ptr_eq(&a.entries, &Replacer::new().entries));
    }
}
