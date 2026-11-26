//! Per-linter options (`linters.settings.misspell`).

/// `linters.settings.misspell` / `linters-settings.misspell`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Regional spelling preference (`US`, `UK`, `GB`, or empty for neutral).
    pub locale: String,
    /// Typos to ignore (case-insensitive).
    pub ignore_words: Vec<String>,
    /// Additional typo → correction pairs.
    pub extra_words: Vec<ExtraWord>,
    /// `restricted` checks comments only; empty/default checks the whole file.
    pub mode: String,
}

/// One `extra-words` entry from golangci-lint YAML.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtraWord {
    pub typo: String,
    pub correction: String,
}

impl Options {
    pub fn restricted(&self) -> bool {
        self.mode.eq_ignore_ascii_case("restricted")
    }
}
