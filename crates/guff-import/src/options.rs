//! Pass-time options for import linters (`linters.settings.*`).

/// `linters.settings.depguard` / `linters-settings.depguard`.
///
/// Empty `rules` → golangci default: one `Main` rule allowing only `$gostd`.
///
/// DEFERRED: path placeholders `${base-path}` / `${config-path}`; full glob
/// library parity for exotic `files` patterns.
#[derive(Debug, Clone, Default)]
pub struct DepguardOptions {
    pub rules: Vec<DepguardRule>,
}

/// One named depguard rule (YAML map key under `rules:`).
#[derive(Debug, Clone)]
pub struct DepguardRule {
    pub name: String,
    /// `original` (default) / `strict` / `lax`.
    pub list_mode: ListMode,
    /// File matchers (`$all`, `$test`, `!$test`, globs). Empty → `$all`.
    pub files: Vec<String>,
    pub allow: Vec<String>,
    pub deny: Vec<DenyEntry>,
}

impl Default for DepguardRule {
    fn default() -> Self {
        Self {
            name: "Main".into(),
            list_mode: ListMode::Original,
            files: Vec::new(),
            allow: vec!["$gostd".into()],
            deny: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListMode {
    #[default]
    Original,
    Strict,
    Lax,
}

impl ListMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "strict" => Self::Strict,
            "lax" => Self::Lax,
            _ => Self::Original,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DenyEntry {
    pub pkg: String,
    pub desc: String,
}

/// `linters.settings.gomoddirectives` / `linters-settings.gomoddirectives`.
///
/// Defaults match golangci-lint (all forbid flags false; empty allow-list).
///
/// DEFERRED: `ignore-forbidden`, `toolchain-pattern`, `go-version-pattern`,
/// `check-module-path`.
#[derive(Debug, Clone, Default)]
pub struct GomoddirectivesOptions {
    /// When true, local `replace` directives are allowed.
    pub replace_local: bool,
    /// Module paths whose `replace` is allowed even when replaces are forbidden.
    pub replace_allow_list: Vec<String>,
    /// When true, `retract` without a rationale comment is allowed.
    pub retract_allow_no_explanation: bool,
    pub exclude_forbidden: bool,
    pub toolchain_forbidden: bool,
    pub tool_forbidden: bool,
    pub go_debug_forbidden: bool,
}

/// `linters.settings.gomodguard` / `gomodguard_v2`.
///
/// Defaults (empty blocked, `local_replace_directives=false`) report nothing.
///
/// DEFERRED: allowed modules/domains, version constraints, `match-type`
/// (`prefix` / `regex`).
#[derive(Clone, Debug, Default)]
pub struct GomodguardOptions {
    /// Blocked module paths (exact / prefix of import module) with reason text.
    pub blocked_modules: Vec<(String, String)>,
    /// When true, imports of modules with a local `replace` are blocked.
    pub local_replace_directives: bool,
}
