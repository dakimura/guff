//! Pass-time options for comment linters (`linters.settings.*`).

/// `linters.settings.godot` / `linters-settings.godot`.
///
/// Defaults match golangci-lint: `scope=declarations`, `period=true`,
/// `capital=false`, empty exclude list.
///
/// DEFERRED: full `toplevel` / `noinline` scopes (unknown scopes fall back to
/// `declarations`); SuggestedFix; block comments inside `const (` / `var (`.
#[derive(Debug, Clone)]
pub struct GodotOptions {
    /// Which comments to check: `declarations`, `all` (others → declarations).
    pub scope: String,
    /// Regexps; matching comment lines are skipped (treated as special).
    pub exclude: Vec<String>,
    /// Require a period at the end of the last sentence.
    pub period: bool,
    /// Require each sentence to start with a capital letter.
    pub capital: bool,
}

impl Default for GodotOptions {
    fn default() -> Self {
        Self {
            scope: "declarations".into(),
            exclude: Vec::new(),
            period: true,
            capital: false,
        }
    }
}

/// `linters.settings.godox` / `linters-settings.godox`.
///
/// Empty `keywords` means golangci defaults: `TODO`, `BUG`, `FIXME`.
#[derive(Debug, Clone)]
pub struct GodoxOptions {
    pub keywords: Vec<String>,
}

impl Default for GodoxOptions {
    fn default() -> Self {
        Self {
            keywords: vec!["TODO".into(), "BUG".into(), "FIXME".into()],
        }
    }
}

impl GodoxOptions {
    /// Effective keyword list (defaults when empty).
    pub fn effective_keywords(&self) -> Vec<String> {
        if self.keywords.is_empty() {
            Self::default().keywords
        } else {
            self.keywords.clone()
        }
    }
}

/// `linters.settings.dupword` / `linters-settings.dupword`.
///
/// Defaults match golangci-lint: empty keyword filter, empty ignore list,
/// `comments-only=false`.
///
/// DEFERRED: SuggestedFix; cross-line duplicate detection spanning adjacent
/// `//` lines; `skip-raw-strings`.
#[derive(Debug, Clone, Default)]
pub struct DupwordOptions {
    /// If non-empty, only these words are flagged as duplicates.
    pub keywords: Vec<String>,
    /// Words to never report (exact match after trailing-comma strip).
    pub ignore: Vec<String>,
    /// When true, skip string literals.
    pub comments_only: bool,
}

/// `linters.settings.godoclint` / `linters-settings.godoclint`.
///
/// Defaults match golangci-lint: `default=basic` (pkg-doc / single-pkg-doc /
/// start-with-name / deprecated), empty enable/disable.
///
/// The four `options.*` fields below are the only ones golangci-lint forwards.
/// Their defaults are godoc-lint's own `config/default.yaml`, which the plain
/// config from golangci-lint layers over field-by-field (`transferIfNotNil`) —
/// so an unset option keeps the upstream default rather than the Rust zero
/// value. `require-doc/ignore-unexported` is the one that differs: it defaults
/// to **true**.
///
/// Every `*/include-tests` option is *pinned* by golangci-lint and a user value
/// for it is discarded; those live as constants in [`crate::godoclint`] because
/// they are not configuration here.
///
/// DEFERRED: `options.max-len.ignore-patterns` is pinned by golangci-lint too
/// (`["^\\+kubebuilder:"]`) and belongs with the `max-len` rule.
#[derive(Debug, Clone)]
pub struct GodoclintOptions {
    /// `basic` (default), `all`, or `none`.
    pub default: String,
    /// Extra rules to enable on top of the default set.
    pub enable: Vec<String>,
    /// Rules to disable.
    pub disable: Vec<String>,
    /// `options.max-len.length` (upstream default 77).
    pub max_len_length: u32,
    /// `options.require-doc.ignore-exported` (upstream default false).
    pub require_doc_ignore_exported: bool,
    /// `options.require-doc.ignore-unexported` (upstream default **true** —
    /// the one default.yaml entry that is not the zero value).
    pub require_doc_ignore_unexported: bool,
    /// `options.start-with-name.include-unexported` (upstream default false).
    pub start_with_name_include_unexported: bool,
}

impl Default for GodoclintOptions {
    fn default() -> Self {
        Self {
            default: "basic".into(),
            enable: Vec::new(),
            disable: Vec::new(),
            max_len_length: 77,
            require_doc_ignore_exported: false,
            require_doc_ignore_unexported: true,
            start_with_name_include_unexported: false,
        }
    }
}

impl GodoclintOptions {
    const BASIC: &'static [&'static str] = &[
        "pkg-doc",
        "single-pkg-doc",
        "start-with-name",
        "deprecated",
    ];

    const ALL: &'static [&'static str] = &[
        "pkg-doc",
        "single-pkg-doc",
        "start-with-name",
        "deprecated",
        "require-doc",
        "require-pkg-doc",
        "max-len",
        "no-unused-link",
        "require-stdlib-doclink",
    ];

    /// Effective rule names after applying `default` / `enable` / `disable`.
    pub fn effective_rules(&self) -> std::collections::HashSet<String> {
        use std::collections::HashSet;
        let mut rules: HashSet<String> = match self.default.as_str() {
            "all" => Self::ALL.iter().map(|s| (*s).to_string()).collect(),
            "none" => HashSet::new(),
            // "basic" and unknown → basic
            _ => Self::BASIC.iter().map(|s| (*s).to_string()).collect(),
        };
        for r in &self.enable {
            rules.insert(r.clone());
        }
        for r in &self.disable {
            rules.remove(r);
        }
        rules
    }
}
