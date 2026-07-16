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
/// DEFERRED: per-rule `options.*`; unimplemented rules (`require-doc`,
/// `require-pkg-doc`, `max-len`, `no-unused-link`, `require-stdlib-doclink`)
/// are accepted in enable/disable for config compat but currently no-op.
#[derive(Debug, Clone)]
pub struct GodoclintOptions {
    /// `basic` (default), `all`, or `none`.
    pub default: String,
    /// Extra rules to enable on top of the default set.
    pub enable: Vec<String>,
    /// Rules to disable.
    pub disable: Vec<String>,
}

impl Default for GodoclintOptions {
    fn default() -> Self {
        Self {
            default: "basic".into(),
            enable: Vec::new(),
            disable: Vec::new(),
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
