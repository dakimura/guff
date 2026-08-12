//! Configuration file parsing and golangci-lint compatibility.
//!
//! Supports reading `.golangci.yml` / `.guff.yml` (v1 and v2) and resolving
//! enabled linters the same way as the CLI `--preset` / `--enable` / `--disable`.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::registry::{known_linter_names, FAST_LINTER_NAMES, STANDARD_LINTER_NAMES};

/// Default config file names searched from the working directory upward.
pub const CONFIG_FILE_NAMES: &[&str] = &[
    ".golangci.yml",
    ".golangci.yaml",
    ".guff.yml",
    ".guff.yaml",
];

/// Linter names moved to the `formatters` section in golangci-lint v2.
pub const FORMATTER_NAMES: &[&str] =
    &["gci", "gofmt", "gofumpt", "goimports", "golines", "swaggo"];

/// Linters removed in golangci-lint v2 (stripped during migration).
pub const DEPRECATED_LINTERS: &[&str] = &[
    "deadcode",
    "execinquery",
    "exhaustivestruct",
    "exportloopref",
    "golint",
    "ifshort",
    "interfacer",
    "maligned",
    "nosnakecase",
    "scopelint",
    "structcheck",
    "tenv",
    "varcheck",
];

/// Preset values for `linters.default`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinterDefault {
    #[default]
    Standard,
    Fast,
    All,
    None,
}

impl LinterDefault {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "standard" => Some(Self::Standard),
            "fast" => Some(Self::Fast),
            "all" => Some(Self::All),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Fast => "fast",
            Self::All => "all",
            Self::None => "none",
        }
    }

    fn base_linters(self) -> &'static [&'static str] {
        match self {
            Self::Standard => STANDARD_LINTER_NAMES,
            // `All` is resolved via `known_linter_names()` in `resolve_names`
            // (includes module plugins); this is only a fallback.
            Self::All => STANDARD_LINTER_NAMES,
            Self::Fast => FAST_LINTER_NAMES,
            Self::None => &[],
        }
    }
}

/// Resolved linter selection used by the CLI driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinterSelection {
    pub default: LinterDefault,
    pub enable: Vec<String>,
    pub disable: Vec<String>,
}

impl Default for LinterSelection {
    fn default() -> Self {
        Self {
            default: LinterDefault::Standard,
            enable: Vec::new(),
            disable: Vec::new(),
        }
    }
}

impl LinterSelection {
    /// Merge CLI overrides on top of a file-based selection.
    pub fn with_cli_overrides(
        mut self,
        cli_default: Option<LinterDefault>,
        enable: &[String],
        disable: &[String],
    ) -> Self {
        if let Some(d) = cli_default {
            self.default = d;
        }
        self.enable.extend(enable.iter().cloned());
        self.disable.extend(disable.iter().cloned());
        self
    }

    /// Compute the final ordered linter name list.
    pub fn resolve_names(&self) -> Vec<String> {
        let mut names: Vec<String> = match self.default {
            LinterDefault::All => known_linter_names()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
            other => other
                .base_linters()
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        };

        let disable: HashSet<&str> = self.disable.iter().map(|s| s.as_str()).collect();
        names.retain(|n| !disable.contains(n.as_str()));

        for e in &self.enable {
            let normalized = normalize_linter_name(e);
            // A linter named in *both* lists stays off. Upstream applies
            // `disable` last, so it wins whichever order the two lists are
            // written in and whatever `default` is — measured against
            // golangci-lint 2.12.2 for `default` standard / none / all.
            // Note this is the opposite of govet's per-analyzer `enable` /
            // `disable`, where `enable` is checked first (see
            // `settings.rs::govet_analyzer_enabled`).
            if disable.contains(normalized) {
                continue;
            }
            if !names.iter().any(|n| n == normalized) {
                names.push(normalized.to_string());
            }
        }

        names
    }
}

/// golangci-lint v2 configuration (subset supported by guff).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigV2 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub linters: LintersV2,
    #[serde(default, skip_serializing_if = "FormattersV2::is_empty")]
    pub formatters: FormattersV2,
    #[serde(default, skip_serializing_if = "IssuesConfig::is_default")]
    pub issues: IssuesConfig,
    #[serde(default, skip_serializing_if = "RunConfig::is_default")]
    pub run: RunConfig,
    #[serde(default, skip_serializing_if = "SeverityConfig::is_default")]
    pub severity: SeverityConfig,
    #[serde(default, skip_serializing_if = "OutputConfig::is_default")]
    pub output: OutputConfig,
}

fn default_true() -> bool {
    true
}

fn default_max_issues_per_linter() -> i32 {
    50
}

fn default_max_same_issues() -> i32 {
    3
}

/// `issues` section (golangci-lint v1 / v2 top-level).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuesConfig {
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default, rename = "exclude-rules")]
    pub exclude_rules: Vec<ExcludeRule>,
    #[serde(default, rename = "exclude-dirs")]
    pub exclude_dirs: Vec<String>,
    #[serde(default, rename = "exclude-files")]
    pub exclude_files: Vec<String>,
    #[serde(default = "default_true", rename = "exclude-use-default")]
    pub exclude_use_default: bool,
    #[serde(default, rename = "exclude-case-sensitive")]
    pub exclude_case_sensitive: bool,
    #[serde(default, rename = "exclude-dirs-use-default")]
    pub exclude_dirs_use_default: Option<bool>,
    #[serde(default = "default_max_issues_per_linter", rename = "max-issues-per-linter")]
    pub max_issues_per_linter: i32,
    #[serde(default = "default_max_same_issues", rename = "max-same-issues")]
    pub max_same_issues: i32,
    #[serde(default, rename = "uniq-by-line")]
    pub uniq_by_line: Option<bool>,
    /// Show only issues in the git working-tree diff (`--new`).
    #[serde(default)]
    pub new: bool,
    #[serde(default, rename = "new-from-rev")]
    pub new_from_rev: Option<String>,
    #[serde(default, rename = "new-from-merge-base")]
    pub new_from_merge_base: Option<String>,
    #[serde(default, rename = "new-from-patch")]
    pub new_from_patch: Option<String>,
    /// When true with a `new*` option, keep issues anywhere in changed files
    /// (not only on changed lines).
    #[serde(default, rename = "whole-files")]
    pub whole_files: bool,
    #[serde(default)]
    pub include: Vec<String>,
    /// From v2 `linters.exclusions.paths-except`: when non-empty, only matching
    /// paths are kept (everything else is excluded).
    #[serde(default, rename = "paths-except", skip_serializing)]
    pub paths_except: Vec<String>,
    /// From v2 `linters.exclusions.generated` (`lax` / `strict` / `disable`).
    #[serde(default, skip_serializing)]
    pub generated: Option<String>,
}

impl Default for IssuesConfig {
    fn default() -> Self {
        Self {
            exclude: Vec::new(),
            exclude_rules: Vec::new(),
            exclude_dirs: Vec::new(),
            exclude_files: Vec::new(),
            exclude_use_default: true,
            exclude_case_sensitive: false,
            exclude_dirs_use_default: None,
            max_issues_per_linter: 50,
            max_same_issues: 3,
            uniq_by_line: None,
            new: false,
            new_from_rev: None,
            new_from_merge_base: None,
            new_from_patch: None,
            whole_files: false,
            include: Vec::new(),
            paths_except: Vec::new(),
            generated: None,
        }
    }
}

impl IssuesConfig {
    fn is_default(&self) -> bool {
        self.exclude.is_empty()
            && self.exclude_rules.is_empty()
            && self.exclude_dirs.is_empty()
            && self.exclude_files.is_empty()
            && self.exclude_use_default
            && !self.exclude_case_sensitive
            && self.exclude_dirs_use_default.is_none()
            && self.max_issues_per_linter == 50
            && self.max_same_issues == 3
            && self.uniq_by_line.is_none()
            && !self.new
            && self.new_from_rev.is_none()
            && self.new_from_merge_base.is_none()
            && self.new_from_patch.is_none()
            && !self.whole_files
            && self.include.is_empty()
            && self.paths_except.is_empty()
            && self.generated.is_none()
    }
}

/// One `issues.exclude-rules` entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExcludeRule {
    #[serde(default)]
    pub linters: Vec<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default, rename = "path-except")]
    pub path_except: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

/// `run` section.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunConfig {
    #[serde(default, rename = "build-tags")]
    pub build_tags: Vec<String>,
    #[serde(default)]
    pub tests: Option<bool>,
    #[serde(default)]
    pub go: Option<String>,
    /// Enforced whole-run deadline (`--timeout` / default `1m`).
    #[serde(default)]
    pub timeout: Option<String>,
    /// Worker count for the action DAG (`-j`). `1` forces sequential; other
    /// values size the rayon thread pool (R9).
    #[serde(default)]
    pub concurrency: Option<i32>,
    #[serde(default, rename = "issues-exit-code")]
    pub issues_exit_code: Option<i32>,
    #[serde(default, rename = "modules-download-mode")]
    pub modules_download_mode: Option<String>,
}

impl RunConfig {
    fn is_default(&self) -> bool {
        self.build_tags.is_empty()
            && self.tests.is_none()
            && self.go.is_none()
            && self.timeout.is_none()
            && self.concurrency.is_none()
            && self.issues_exit_code.is_none()
            && self.modules_download_mode.is_none()
    }
}

/// `severity` section.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeverityConfig {
    /// v1 spelling. v2 renamed the key to `default` (see [`Self::default_v2`]),
    /// and the two must stay apart: reading the v1 name out of a v2 config would
    /// apply a default golangci-lint rejects the file for.
    #[serde(default, rename = "default-severity")]
    pub default_severity: Option<String>,
    /// v2 spelling of the same key (`severity.default`).
    #[serde(default, rename = "default", skip_serializing)]
    pub default_v2: Option<String>,
    #[serde(default, rename = "case-sensitive")]
    pub case_sensitive: bool,
    #[serde(default)]
    pub rules: Vec<SeverityRule>,
}

impl SeverityConfig {
    fn is_default(&self) -> bool {
        self.default_severity.is_none()
            && self.default_v2.is_none()
            && !self.case_sensitive
            && self.rules.is_empty()
    }
}

/// One `severity.rules` entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeverityRule {
    #[serde(default)]
    pub linters: Vec<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default, rename = "path-except")]
    pub path_except: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub severity: String,
}

/// `output` section. `formats` / deprecated `format` feed `--out-format` resolution (R6).
/// JSON and other formatters: R7/R8.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputConfig {
    #[serde(default)]
    pub formats: serde_yaml::Value,
    #[serde(default, rename = "print-issued-lines")]
    pub print_issued_lines: Option<bool>,
    #[serde(default, rename = "print-linter-name")]
    pub print_linter_name: Option<bool>,
    #[serde(default, rename = "sort-results")]
    pub sort_results: Option<bool>,
    #[serde(default, rename = "path-prefix")]
    pub path_prefix: Option<String>,
    /// golangci `output.path-mode`: empty/`rel` (default) or `abs`.
    #[serde(default, rename = "path-mode")]
    pub path_mode: Option<String>,
    #[serde(default, rename = "show-stats")]
    pub show_stats: Option<bool>,
    /// Deprecated single-format string.
    #[serde(default)]
    pub format: Option<String>,
}

impl OutputConfig {
    fn is_default(&self) -> bool {
        self.formats.is_null()
            && self.print_issued_lines.is_none()
            && self.print_linter_name.is_none()
            && self.sort_results.is_none()
            && self.path_prefix.is_none()
            && self.path_mode.is_none()
            && self.show_stats.is_none()
            && self.format.is_none()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LintersV2 {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default)]
    pub disable: Vec<String>,
    #[serde(default, skip_serializing_if = "LinterExclusions::is_default")]
    pub exclusions: LinterExclusions,
    #[serde(default, skip_serializing_if = "serde_yaml::Value::is_null")]
    pub settings: serde_yaml::Value,
}

/// golangci-lint v2 `linters.exclusions` (replaces v1 `issues.exclude*`).
///
/// See https://golangci-lint.run/docs/linters/false-positives/
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LinterExclusions {
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default, rename = "paths-except")]
    pub paths_except: Vec<String>,
    #[serde(default)]
    pub rules: Vec<ExcludeRule>,
    /// Named exclusion presets (`comments`, `stdErrorHandling`, …).
    /// Accepted under both `presets` (docs) and `preset` (Go mapstructure).
    #[serde(default, alias = "preset")]
    pub presets: Vec<String>,
    #[serde(default, rename = "warn-unused")]
    pub warn_unused: bool,
    /// Skip issues in generated files: `lax` / `strict` / `disable`.
    #[serde(default)]
    pub generated: Option<String>,
}

impl LinterExclusions {
    fn is_default(&self) -> bool {
        self.paths.is_empty()
            && self.paths_except.is_empty()
            && self.rules.is_empty()
            && self.presets.is_empty()
            && !self.warn_unused
            && self.generated.is_none()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FormattersV2 {
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default, skip_serializing_if = "serde_yaml::Value::is_null")]
    pub settings: serde_yaml::Value,
    #[serde(default, skip_serializing_if = "FormatterExclusions::is_empty")]
    pub exclusions: FormatterExclusions,
}

/// `formatters.exclusions` (paths + generated mode + warn-unused stub).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FormatterExclusions {
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default, rename = "warn-unused")]
    pub warn_unused: bool,
    /// golangci: `lax` (default) / `strict` / `disable`.
    #[serde(default)]
    pub generated: Option<String>,
}

impl FormatterExclusions {
    fn is_empty(&self) -> bool {
        self.paths.is_empty() && !self.warn_unused && self.generated.is_none()
    }
}

impl FormattersV2 {
    fn is_empty(&self) -> bool {
        self.enable.is_empty() && self.settings.is_null() && self.exclusions.is_empty()
    }

    /// Parse `formatters.settings.gofmt` into [`guff_fmt::GofmtOptions`].
    pub fn gofmt_options(&self) -> guff_fmt::GofmtOptions {
        parse_gofmt_settings(&self.settings)
    }

    /// Parse `formatters.settings.gofumpt` into [`guff_fmt::GofumptOptions`].
    pub fn gofumpt_options(&self) -> guff_fmt::GofumptOptions {
        parse_gofumpt_settings(&self.settings)
    }

    /// Parse `formatters.settings.goimports` into [`guff_fmt::GoimportsOptions`].
    pub fn goimports_options(&self) -> guff_fmt::GoimportsOptions {
        parse_goimports_settings(&self.settings)
    }

    /// Parse `formatters.settings.gci` into [`guff_fmt::GciOptions`].
    pub fn gci_options(&self) -> guff_fmt::GciOptions {
        parse_gci_settings(&self.settings)
    }

    /// Parse `formatters.settings.golines` into [`guff_fmt::GolinesOptions`].
    pub fn golines_options(&self) -> guff_fmt::GolinesOptions {
        parse_golines_settings(&self.settings)
    }

    /// Path exclusion patterns from `formatters.exclusions.paths`.
    pub fn exclusion_paths(&self) -> Vec<String> {
        self.exclusions.paths.clone()
    }

    /// `formatters.exclusions.generated` (`lax` when unset).
    pub fn exclusion_generated(&self) -> guff_fmt::GeneratedMode {
        guff_fmt::GeneratedMode::parse(self.exclusions.generated.as_deref())
    }
}

/// Parse gofmt settings from `formatters.settings` YAML mapping.
pub fn parse_gofmt_settings(settings: &serde_yaml::Value) -> guff_fmt::GofmtOptions {
    let mut opts = guff_fmt::GofmtOptions::default();
    let Some(map) = settings.as_mapping() else {
        return opts;
    };
    let Some(gofmt) = map.get(serde_yaml::Value::String("gofmt".into())) else {
        return opts;
    };
    let Some(gmap) = gofmt.as_mapping() else {
        return opts;
    };
    if let Some(v) = gmap.get(serde_yaml::Value::String("simplify".into())) {
        if let Some(b) = v.as_bool() {
            opts.simplify = b;
        }
    }
    if let Some(rules) = gmap.get(serde_yaml::Value::String("rewrite-rules".into())) {
        if let Some(seq) = rules.as_sequence() {
            for item in seq {
                let Some(rmap) = item.as_mapping() else {
                    continue;
                };
                let pattern = rmap
                    .get(serde_yaml::Value::String("pattern".into()))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let replacement = rmap
                    .get(serde_yaml::Value::String("replacement".into()))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !pattern.is_empty() {
                    opts.rewrite_rules.push(guff_fmt::RewriteRule {
                        pattern,
                        replacement,
                    });
                }
            }
        }
    }
    opts
}

/// Parse gofumpt settings from `formatters.settings` YAML mapping.
pub fn parse_gofumpt_settings(settings: &serde_yaml::Value) -> guff_fmt::GofumptOptions {
    let mut opts = guff_fmt::GofumptOptions::default();
    let Some(map) = settings.as_mapping() else {
        return opts;
    };
    let Some(gofumpt) = map.get(serde_yaml::Value::String("gofumpt".into())) else {
        return opts;
    };
    let Some(gmap) = gofumpt.as_mapping() else {
        return opts;
    };
    if let Some(v) = gmap.get(serde_yaml::Value::String("extra-rules".into())) {
        if let Some(b) = v.as_bool() {
            opts.extra_rules = b;
        }
    }
    if let Some(v) = gmap.get(serde_yaml::Value::String("module-path".into())) {
        if let Some(s) = v.as_str() {
            if !s.is_empty() {
                opts.module_path = Some(s.to_string());
            }
        }
    }
    // Non-standard convenience key; the CLI also injects `-lang` from `run.go`.
    if let Some(v) = gmap.get(serde_yaml::Value::String("lang".into())) {
        if let Some(s) = v.as_str() {
            if !s.is_empty() {
                opts.lang = Some(s.to_string());
            }
        }
    }
    opts
}

/// Parse goimports settings from `formatters.settings` YAML mapping.
pub fn parse_goimports_settings(settings: &serde_yaml::Value) -> guff_fmt::GoimportsOptions {
    let mut opts = guff_fmt::GoimportsOptions::default();
    let Some(map) = settings.as_mapping() else {
        return opts;
    };
    let Some(goimports) = map.get(serde_yaml::Value::String("goimports".into())) else {
        return opts;
    };
    let Some(gmap) = goimports.as_mapping() else {
        return opts;
    };
    if let Some(v) = gmap.get(serde_yaml::Value::String("local-prefixes".into())) {
        match v {
            serde_yaml::Value::Sequence(seq) => {
                for item in seq {
                    if let Some(s) = item.as_str() {
                        if !s.is_empty() {
                            opts.local_prefixes.push(s.to_string());
                        }
                    }
                }
            }
            // golangci also accepts a comma-separated string in some configs.
            serde_yaml::Value::String(s) => {
                for part in s.split(',') {
                    let part = part.trim();
                    if !part.is_empty() {
                        opts.local_prefixes.push(part.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    opts
}

/// Parse gci settings from `formatters.settings` YAML mapping.
pub fn parse_gci_settings(settings: &serde_yaml::Value) -> guff_fmt::GciOptions {
    let mut opts = guff_fmt::GciOptions::default();
    let Some(map) = settings.as_mapping() else {
        return opts;
    };
    let Some(gci) = map.get(serde_yaml::Value::String("gci".into())) else {
        return opts;
    };
    let Some(gmap) = gci.as_mapping() else {
        return opts;
    };
    if let Some(v) = gmap.get(serde_yaml::Value::String("sections".into())) {
        let mut sections = Vec::new();
        match v {
            serde_yaml::Value::Sequence(seq) => {
                for item in seq {
                    if let Some(s) = item.as_str() {
                        if !s.is_empty() {
                            sections.push(s.to_string());
                        }
                    }
                }
            }
            serde_yaml::Value::String(s) => {
                for part in s.split(',') {
                    let part = part.trim();
                    if !part.is_empty() {
                        sections.push(part.to_string());
                    }
                }
            }
            _ => {}
        }
        if !sections.is_empty() {
            opts.sections = sections;
        }
    }
    if let Some(v) = gmap.get(serde_yaml::Value::String("custom-order".into())) {
        if let Some(b) = v.as_bool() {
            opts.custom_order = b;
        }
    }
    if let Some(v) = gmap.get(serde_yaml::Value::String("no-lex-order".into())) {
        if let Some(b) = v.as_bool() {
            opts.no_lex_order = b;
        }
    }
    // Post-processed in guff-fmt (gci `print` CLI does not expose these).
    if let Some(v) = gmap.get(serde_yaml::Value::String("no-inline-comments".into())) {
        if let Some(b) = v.as_bool() {
            opts.no_inline_comments = b;
        }
    }
    if let Some(v) = gmap.get(serde_yaml::Value::String("no-prefix-comments".into())) {
        if let Some(b) = v.as_bool() {
            opts.no_prefix_comments = b;
        }
    }
    opts
}

/// Parse golines settings from `formatters.settings` YAML mapping.
pub fn parse_golines_settings(settings: &serde_yaml::Value) -> guff_fmt::GolinesOptions {
    let mut opts = guff_fmt::GolinesOptions::default();
    let Some(map) = settings.as_mapping() else {
        return opts;
    };
    let Some(golines) = map.get(serde_yaml::Value::String("golines".into())) else {
        return opts;
    };
    let Some(gmap) = golines.as_mapping() else {
        return opts;
    };
    if let Some(v) = gmap.get(serde_yaml::Value::String("max-len".into())) {
        if let Some(n) = yaml_u32(v) {
            opts.max_len = n;
        }
    }
    if let Some(v) = gmap.get(serde_yaml::Value::String("tab-len".into())) {
        if let Some(n) = yaml_u32(v) {
            opts.tab_len = n;
        }
    }
    if let Some(v) = gmap.get(serde_yaml::Value::String("shorten-comments".into())) {
        if let Some(b) = v.as_bool() {
            opts.shorten_comments = b;
        }
    }
    if let Some(v) = gmap.get(serde_yaml::Value::String("reformat-tags".into())) {
        if let Some(b) = v.as_bool() {
            opts.reformat_tags = b;
        }
    }
    if let Some(v) = gmap.get(serde_yaml::Value::String("chain-split-dots".into())) {
        if let Some(b) = v.as_bool() {
            opts.chain_split_dots = b;
        }
    }
    opts
}

fn yaml_u32(v: &serde_yaml::Value) -> Option<u32> {
    if let Some(n) = v.as_u64() {
        return u32::try_from(n).ok();
    }
    if let Some(n) = v.as_i64() {
        if n >= 0 {
            return u32::try_from(n).ok();
        }
    }
    None
}

/// golangci-lint v1 configuration (subset supported for migration).
#[derive(Debug, Clone, Default, Deserialize)]
struct ConfigV1 {
    #[serde(default)]
    linters: LintersV1,
    #[serde(default, rename = "linters-settings")]
    linters_settings: serde_yaml::Value,
    #[serde(default)]
    issues: IssuesConfig,
    #[serde(default)]
    run: RunConfig,
    #[serde(default)]
    severity: SeverityConfig,
    #[serde(default)]
    output: OutputConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct LintersV1 {
    #[serde(default, rename = "enable-all")]
    enable_all: bool,
    #[serde(default, rename = "disable-all")]
    disable_all: bool,
    #[serde(default)]
    enable: Vec<String>,
    #[serde(default)]
    disable: Vec<String>,
    #[serde(default)]
    presets: Vec<String>,
}

/// Errors while loading or parsing configuration.
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(serde_yaml::Error),
    Migrate(String),
    NotFound,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Parse(e) => write!(f, "invalid config: {e}"),
            Self::Migrate(msg) => write!(f, "{msg}"),
            Self::NotFound => write!(f, "no configuration file found"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_yaml::Error> for ConfigError {
    fn from(value: serde_yaml::Error) -> Self {
        Self::Parse(value)
    }
}

/// Discover a config file starting at `start` and walking up to the filesystem root.
pub fn discover_config(start: &Path) -> Option<PathBuf> {
    let mut dir = start.canonicalize().ok()?;
    loop {
        for name in CONFIG_FILE_NAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Load configuration from `path`. Accepts golangci v1 or v2 YAML.
pub fn load_config(path: &Path) -> Result<ConfigFile, ConfigError> {
    let contents = std::fs::read_to_string(path)?;
    parse_config_str(&contents)
}

/// Parse configuration YAML from a string.
pub fn parse_config_str(contents: &str) -> Result<ConfigFile, ConfigError> {
    let raw: serde_yaml::Value = serde_yaml::from_str(contents)?;
    if is_v2(&raw) {
        let cfg: ConfigV2 = serde_yaml::from_value(raw)?;
        Ok(ConfigFile::V2(cfg))
    } else {
        let cfg: ConfigV1 = serde_yaml::from_str(contents)?;
        Ok(ConfigFile::V1(cfg))
    }
}

/// Loaded configuration (v1 or v2).
#[derive(Debug, Clone)]
pub enum ConfigFile {
    V1(ConfigV1),
    V2(ConfigV2),
}

impl ConfigFile {
    pub fn linter_selection(&self) -> LinterSelection {
        match self {
            Self::V1(v1) => v1.linter_selection(),
            Self::V2(v2) => v2.linter_selection(),
        }
    }

    pub fn issues(&self) -> &IssuesConfig {
        match self {
            Self::V1(v1) => &v1.issues,
            Self::V2(v2) => &v2.issues,
        }
    }

    /// Issues config with v2 `linters.exclusions` folded in (golangci v2 shape).
    ///
    /// V2 has no default exclusions unless `linters.exclusions.presets` is set.
    /// `linters.exclusions.paths` / `rules` / `paths-except` are merged into the
    /// filter inputs that [`crate::exclude::IssueFilter`] already understands.
    pub fn effective_issues(&self) -> IssuesConfig {
        match self {
            Self::V1(v1) => v1.issues.clone(),
            Self::V2(v2) => merge_v2_exclusions(&v2.issues, &v2.linters.exclusions),
        }
    }

    pub fn exclusions(&self) -> Option<&LinterExclusions> {
        match self {
            Self::V1(_) => None,
            Self::V2(v2) => Some(&v2.linters.exclusions),
        }
    }

    pub fn run(&self) -> &RunConfig {
        match self {
            Self::V1(v1) => &v1.run,
            Self::V2(v2) => &v2.run,
        }
    }

    pub fn severity(&self) -> &SeverityConfig {
        match self {
            Self::V1(v1) => &v1.severity,
            Self::V2(v2) => &v2.severity,
        }
    }

    /// Severity config as the post-processor needs it.
    ///
    /// Same story as [`Self::effective_issues`]: v2's `severity` section is
    /// `{default, rules}` and nothing else, and its rules are compiled with an
    /// empty regex prefix, so the v1-only `case-sensitive` key must read as
    /// true rather than as serde's default false.
    ///
    /// The default severity is spelled `default-severity` in v1 and `default`
    /// in v2, and only the version's own spelling counts — a v2 config that
    /// says `default-severity` is one golangci-lint refuses to load, not one
    /// with a default.
    pub fn effective_severity(&self) -> SeverityConfig {
        match self {
            Self::V1(v1) => v1.severity.clone(),
            Self::V2(v2) => SeverityConfig {
                case_sensitive: true,
                default_severity: v2.severity.default_v2.clone(),
                ..v2.severity.clone()
            },
        }
    }

    pub fn output(&self) -> &OutputConfig {
        match self {
            Self::V1(v1) => &v1.output,
            Self::V2(v2) => &v2.output,
        }
    }

    /// `formatters` section (v2). For v1, migrates linters that moved to formatters.
    pub fn formatters(&self) -> FormattersV2 {
        match self {
            Self::V2(v2) => v2.formatters.clone(),
            Self::V1(v1) => migrate_v1_to_v2(v1).formatters,
        }
    }

    /// Raw `linters.settings` (v2) or `linters-settings` (v1) YAML.
    pub fn linter_settings_raw(&self) -> &serde_yaml::Value {
        match self {
            Self::V1(v1) => &v1.linters_settings,
            Self::V2(v2) => &v2.linters.settings,
        }
    }

    /// Deterministic one-line rendering of [`Self::linter_settings_raw`],
    /// for the issues-cache salt.
    ///
    /// Mappings are emitted in sorted-key order so the same config always
    /// produces the same string — `serde_yaml::Mapping` preserves insertion
    /// order, but the salt must not depend on how the user ordered their YAML.
    pub fn linter_settings_fingerprint(&self) -> String {
        canonical_yaml(self.linter_settings_raw())
    }

    pub fn is_v1(&self) -> bool {
        matches!(self, Self::V1(_))
    }

    pub fn is_v2(&self) -> bool {
        matches!(self, Self::V2(_))
    }
}

impl ConfigV2 {
    pub fn linter_selection(&self) -> LinterSelection {
        let default = self
            .linters
            .default
            .as_deref()
            .and_then(LinterDefault::parse)
            .unwrap_or(LinterDefault::Standard);

        LinterSelection {
            default,
            enable: self.linters.enable.clone(),
            disable: self.linters.disable.clone(),
        }
    }
}

impl ConfigV1 {
    fn linter_selection(&self) -> LinterSelection {
        let default = if self.linters.disable_all {
            LinterDefault::None
        } else if self.linters.enable_all {
            LinterDefault::All
        } else {
            LinterDefault::Standard
        };

        let mut enable = self.linters.enable.clone();
        enable.extend(preset_linters(&self.linters.presets));

        LinterSelection {
            default,
            enable,
            disable: self.linters.disable.clone(),
        }
    }
}

/// Normalize golangci linter aliases to canonical names.
pub fn normalize_linter_name(name: &str) -> &str {
    match name {
        "gas" => "gosec",
        "goerr113" => "err113",
        "gomnd" => "mnd",
        "logrlint" => "loggercheck",
        "megacheck" | "gosimple" | "stylecheck" => "staticcheck",
        "vet" | "vetshadow" => "govet",
        "deadcode" | "structcheck" | "varcheck" => "unused",
        "typecheck" => "typecheck",
        other => other,
    }
}

fn is_v2(raw: &serde_yaml::Value) -> bool {
    raw.get("version")
        .and_then(|v| v.as_str())
        .is_some_and(|v| v == "2")
}

/// Fold v2 `linters.exclusions` into an [`IssuesConfig`] for the post-process filter.
fn merge_v2_exclusions(issues: &IssuesConfig, excl: &LinterExclusions) -> IssuesConfig {
    let mut out = issues.clone();

    // golangci-lint v2: no default exclusions. Serde still defaults
    // `issues.exclude-use-default` to true when the key is absent, so force
    // false here. Presets expand into the corresponding EXC* patterns.
    out.exclude_use_default = false;

    // v2 compiles every exclusion rule with an *empty* regex prefix
    // (`parseRules(excludeRules, "", newExcludeRule)`), and its `issues`
    // section has no `exclude-case-sensitive` key at all — the knob was left
    // behind in v1. Serde still defaults the field to false, which would put
    // `(?i)` in front of every `text:` and `source:` pattern and quietly widen
    // them: `text: ERROR RETURN VALUE` matched every errcheck finding here and
    // none upstream.
    out.exclude_case_sensitive = true;
    if !excl.presets.is_empty() {
        apply_exclusion_presets(&mut out, &excl.presets);
    }

    if out.exclude_dirs_use_default.is_none() {
        out.exclude_dirs_use_default = Some(false);
    }

    out.exclude_files.extend(excl.paths.iter().cloned());
    out.exclude_rules.extend(excl.rules.iter().cloned());
    out.paths_except.extend(excl.paths_except.iter().cloned());

    // `linters.exclusions.generated` defaults to **strict**, not to lax:
    // `config.Loader.Load` fills the empty value in with `GeneratedModeStrict`
    // before any processor sees it. Only `formatters.exclusions.generated` is
    // left empty, and empty means lax in the matcher — so `golangci-lint run`
    // and `golangci-lint fmt` disagree about what a generated file is, and
    // both behaviours are upstream's.
    //
    // Defaulting to lax here dropped findings from any file whose leading
    // comments merely contain "do not edit" or "autogenerated file" anywhere,
    // in prose, which is a much larger set than the one-line convention.
    out.generated = Some(
        excl.generated
            .clone()
            .unwrap_or_else(|| "strict".to_string()),
    );
    // DEFERRED: warn-unused (report unused exclusion rules).
    let _ = excl.warn_unused;

    out
}

/// Expand golangci v2 exclusion presets into default exclude patterns.
///
/// Preset names follow golangci (`comments`, `stdErrorHandling`,
/// `commonFalsePositives`, `legacy`) and also accept kebab-case aliases
/// (`std-error-handling`, …) used in docs.
fn apply_exclusion_presets(issues: &mut IssuesConfig, presets: &[String]) {
    let wanted: std::collections::HashSet<&str> = presets
        .iter()
        .map(|p| normalize_exclusion_preset(p.as_str()))
        .collect();

    for pat in default_exclude_patterns_for_presets() {
        if !wanted.contains(pat.preset) {
            continue;
        }
        issues.exclude_rules.push(ExcludeRule {
            linters: vec![pat.linter.to_string()],
            text: Some(pat.pattern.to_string()),
            ..ExcludeRule::default()
        });
    }
}

fn normalize_exclusion_preset(name: &str) -> &str {
    match name {
        "comments" => "comments",
        "stdErrorHandling" | "std-error-handling" => "stdErrorHandling",
        "commonFalsePositives" | "common-false-positives" => "commonFalsePositives",
        "legacy" => "legacy",
        other => other,
    }
}

struct PresetExcludePattern {
    preset: &'static str,
    /// golangci's own id for the rule (`InternalReference`), for cross-reading
    /// against `processors.LinterExclusionPresets`.
    id: &'static str,
    linter: &'static str,
    pattern: &'static str,
}

/// golangci-lint v2 `processors.LinterExclusionPresets`, rule for rule.
///
/// This is **not** the v1 `EXC0001`…`EXC0015` list that
/// [`crate::exclude::default_exclude_patterns`] still holds for
/// `issues.exclude-use-default`. v2 dropped EXC0002 and EXC0003 (they named
/// `golint`, which v2 does not have) and moved EXC0011 from `stylecheck` to
/// `staticcheck`. Since the linter name is compared verbatim against
/// `issue.FromLinter`, a leftover v1 entry is either inert or an
/// over-exclusion — never a match upstream also makes.
///
/// The `(?i)` in EXC0001 is upstream's, written into that one pattern.
/// Exclusion rules are compiled with an empty prefix in v2, so it is the only
/// case-insensitive rule in the set.
fn default_exclude_patterns_for_presets() -> &'static [PresetExcludePattern] {
    static PATTERNS: &[PresetExcludePattern] = &[
        // comments
        PresetExcludePattern {
            preset: "comments",
            id: "EXC0011",
            linter: "staticcheck",
            pattern: r"(ST1000|ST1020|ST1021|ST1022)",
        },
        PresetExcludePattern {
            preset: "comments",
            id: "EXC0012",
            linter: "revive",
            pattern: r"exported (.+) should have comment( \(or a comment on this block\))? or be unexported",
        },
        PresetExcludePattern {
            preset: "comments",
            id: "EXC0013",
            linter: "revive",
            pattern: r#"package comment should be of the form "(.+)...""#,
        },
        PresetExcludePattern {
            preset: "comments",
            id: "EXC0014",
            linter: "revive",
            pattern: r#"comment on exported (.+) should be of the form "(.+)...""#,
        },
        PresetExcludePattern {
            preset: "comments",
            id: "EXC0015",
            linter: "revive",
            pattern: "should have a package comment",
        },
        // stdErrorHandling
        PresetExcludePattern {
            preset: "stdErrorHandling",
            id: "EXC0001",
            linter: "errcheck",
            pattern: r"(?i)Error return value of .((os\.)?std(out|err)\..*|.*Close|.*Flush|os\.Remove(All)?|.*print(f|ln)?|os\.(Un)?Setenv). is not checked",
        },
        // commonFalsePositives
        PresetExcludePattern {
            preset: "commonFalsePositives",
            id: "EXC0006",
            linter: "gosec",
            pattern: "G103: Use of unsafe calls should be audited",
        },
        PresetExcludePattern {
            preset: "commonFalsePositives",
            id: "EXC0007",
            linter: "gosec",
            pattern: "G204: Subprocess launched with variable",
        },
        PresetExcludePattern {
            preset: "commonFalsePositives",
            id: "EXC0010",
            linter: "gosec",
            pattern: "G304: Potential file inclusion via variable",
        },
        // legacy
        PresetExcludePattern {
            preset: "legacy",
            id: "EXC0004",
            linter: "govet",
            pattern: r"(possible misuse of unsafe.Pointer|should have signature)",
        },
        PresetExcludePattern {
            preset: "legacy",
            id: "EXC0005",
            linter: "staticcheck",
            pattern: "SA4011",
        },
        PresetExcludePattern {
            preset: "legacy",
            id: "EXC0008",
            linter: "gosec",
            pattern: "G104",
        },
        PresetExcludePattern {
            preset: "legacy",
            id: "EXC0009",
            linter: "gosec",
            pattern: r"(G301|G302|G307): Expect (directory permissions to be 0750|file permissions to be 0600) or less",
        },
    ];
    PATTERNS
}

/// Render a YAML value with mapping keys in sorted order.
///
/// Used for the issues-cache salt, so the only requirements are determinism
/// and injectivity-in-practice; the output is never parsed back.
fn canonical_yaml(value: &serde_yaml::Value) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    fn write(out: &mut String, value: &serde_yaml::Value) {
        match value {
            serde_yaml::Value::Null => out.push_str("~"),
            serde_yaml::Value::Bool(b) => {
                let _ = write!(out, "{b}");
            }
            serde_yaml::Value::Number(n) => {
                let _ = write!(out, "{n}");
            }
            serde_yaml::Value::String(s) => {
                let _ = write!(out, "{s:?}");
            }
            serde_yaml::Value::Sequence(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(out, item);
                }
                out.push(']');
            }
            serde_yaml::Value::Mapping(map) => {
                let mut entries: Vec<(String, &serde_yaml::Value)> = map
                    .iter()
                    .map(|(k, v)| {
                        let mut key = String::new();
                        write(&mut key, k);
                        (key, v)
                    })
                    .collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                out.push('{');
                for (i, (key, val)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(key);
                    out.push(':');
                    write(out, val);
                }
                out.push('}');
            }
            serde_yaml::Value::Tagged(tagged) => {
                let _ = write!(out, "{}", tagged.tag);
                write(out, &tagged.value);
            }
        }
    }
    write(&mut out, value);
    out
}

fn preset_linters(presets: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for preset in presets {
        let linters = match preset.as_str() {
            "bugs" => BUGS_PRESET,
            "comment" => COMMENT_PRESET,
            "complexity" => COMPLEXITY_PRESET,
            "error" => ERROR_PRESET,
            "format" => FORMAT_PRESET,
            "import" => IMPORT_PRESET,
            "metalinter" => METALINTER_PRESET,
            "module" => MODULE_PRESET,
            "performance" => PERFORMANCE_PRESET,
            "sql" => SQL_PRESET,
            "style" => STYLE_PRESET,
            "test" => TEST_PRESET,
            "unused" => UNUSED_PRESET,
            _ => &[],
        };
        for l in linters {
            if !out.iter().any(|x: &String| x == l) {
                out.push((*l).to_string());
            }
        }
    }
    out
}

// golangci-lint v1 preset tables (for migration of `linters.presets`).
const BUGS_PRESET: &[&str] = &[
    "asasalint", "asciicheck", "bidichk", "bodyclose", "contextcheck", "durationcheck",
    "errcheck", "errchkjson", "errorlint", "exhaustive", "gocheckcompilerdirectives",
    "gochecksumtype", "gosec", "gosmopolitan", "govet", "loggercheck", "makezero",
    "musttag", "nilerr", "nilnesserr", "noctx", "protogetter", "reassign", "recvcheck",
    "rowserrcheck", "spancheck", "sqlclosecheck", "staticcheck", "testifylint",
    "zerologlint",
];
const COMMENT_PRESET: &[&str] = &["dupword", "godoclint", "godot", "godox", "misspell"];
const COMPLEXITY_PRESET: &[&str] =
    &["cyclop", "funlen", "gocognit", "gocyclo", "maintidx", "nestif"];
const ERROR_PRESET: &[&str] = &["err113", "errcheck", "errorlint", "wrapcheck"];
const FORMAT_PRESET: &[&str] = &["gci", "gofmt", "gofumpt", "goimports"];
const IMPORT_PRESET: &[&str] = &["depguard", "gci", "goimports", "gomodguard"];
const METALINTER_PRESET: &[&str] = &["gocritic", "govet", "revive", "staticcheck"];
const MODULE_PRESET: &[&str] = &["depguard", "gomoddirectives", "gomodguard"];
const PERFORMANCE_PRESET: &[&str] = &["bodyclose", "fatcontext", "noctx", "perfsprint", "prealloc"];
const SQL_PRESET: &[&str] = &["rowserrcheck", "sqlclosecheck", "unqueryvet"];
const STYLE_PRESET: &[&str] = &[
    "asciicheck", "canonicalheader", "containedctx", "copyloopvar", "decorder", "depguard",
    "dogsled", "dupl", "embeddedstructfieldcheck", "err113", "errname", "exhaustruct", "exptostd",
    "forbidigo",
    "forcetypeassert", "ginkgolinter", "gochecknoglobals", "gochecknoinits", "goconst",
    "gocritic", "godoclint", "godot", "godox", "goheader", "gomoddirectives", "gomodguard",
    "goprintffuncname", "gosimple", "grouper", "iface", "importas", "inamedparam",
    "interfacebloat", "intrange", "iotamixing", "ireturn", "lll", "loggercheck", "makezero", "mirror",
    "misspell", "mnd", "musttag", "nakedret", "nilnil", "nlreturn", "nolintlint",
    "nonamedreturns", "nosprintfhostport", "paralleltest", "predeclared", "promlinter",
    "revive", "sloglint", "stylecheck", "tagalign", "tagliatelle", "testpackage",
    "tparallel", "unconvert", "usestdlibvars", "varnamelen", "wastedassign", "whitespace",
    "wrapcheck", "wsl", "wsl_v5",
];
const TEST_PRESET: &[&str] = &[
    "exhaustruct", "paralleltest", "testableexamples", "testifylint", "testpackage",
    "thelper", "tparallel", "usetesting",
];
const UNUSED_PRESET: &[&str] = &["ineffassign", "unparam", "unused"];

/// Migrate a v1 config file to v2 format.
pub(crate) fn migrate_v1_to_v2(v1: &ConfigV1) -> ConfigV2 {
    let default = if v1.linters.disable_all {
        Some("none".to_string())
    } else if v1.linters.enable_all {
        Some("all".to_string())
    } else {
        None
    };

    let mut enable: Vec<String> = v1.linters.enable.clone();
    enable.extend(preset_linters(&v1.linters.presets));
    enable = dedupe_normalized(enable);

    let mut formatters_enable = Vec::new();
    enable.retain(|name| {
        if FORMATTER_NAMES.contains(&name.as_str()) {
            formatters_enable.push(name.clone());
            false
        } else {
            true
        }
    });

    enable.retain(|name| !DEPRECATED_LINTERS.contains(&name.as_str()));
    enable.retain(|name| name != "typecheck");

    let disable: Vec<String> = v1
        .linters
        .disable
        .iter()
        .map(|n| normalize_linter_name(n).to_string())
        .collect();

    let (linter_settings, formatter_settings) = if v1.linters_settings.is_null() {
        (serde_yaml::Value::Null, serde_yaml::Value::Null)
    } else {
        split_settings(&v1.linters_settings, &mut formatters_enable)
    };

    ConfigV2 {
        version: Some("2".to_string()),
        linters: LintersV2 {
            default,
            enable,
            disable,
            exclusions: LinterExclusions::default(),
            settings: linter_settings,
        },
        formatters: FormattersV2 {
            enable: dedupe_normalized(formatters_enable),
            settings: formatter_settings,
            exclusions: FormatterExclusions::default(),
        },
        issues: v1.issues.clone(),
        run: v1.run.clone(),
        severity: v1.severity.clone(),
        output: v1.output.clone(),
    }
}

/// Migrate configuration from a file path, writing backup and migrated output.
pub fn migrate_config_file(path: &Path, skip_validation: bool) -> Result<ConfigV2, ConfigError> {
    let contents = std::fs::read_to_string(path)?;
    let cfg = parse_config_str(&contents)?;

    let v1 = match cfg {
        ConfigFile::V2(v2) => {
            if skip_validation {
                return Ok(v2);
            }
            return Err(ConfigError::Migrate(
                "configuration is already v2 (version: \"2\")".into(),
            ));
        }
        ConfigFile::V1(v1) => v1,
    };

    let migrated = migrate_v1_to_v2(&v1);

    let backup = backup_path(path);
    std::fs::copy(path, &backup)?;

    let out = serde_yaml::to_string(&migrated)?;
    std::fs::write(path, out)?;

    Ok(migrated)
}

/// Path for the backup file created during migration.
pub fn backup_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("config");
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("yml");
    path.with_file_name(format!("{stem}.bck.{ext}"))
}

fn dedupe_normalized(mut names: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    names.retain_mut(|name| {
        let n = normalize_linter_name(name);
        if n == "typecheck" {
            return false;
        }
        if seen.insert(n.to_string()) {
            *name = n.to_string();
            true
        } else {
            false
        }
    });
    names
}

fn split_settings(
    settings: &serde_yaml::Value,
    formatters_enable: &mut Vec<String>,
) -> (serde_yaml::Value, serde_yaml::Value) {
    let Some(map) = settings.as_mapping() else {
        return (settings.clone(), serde_yaml::Value::Null);
    };

    let mut linter_settings = serde_yaml::Mapping::new();
    let mut formatter_settings = serde_yaml::Mapping::new();

    for (key, value) in map {
        let Some(name) = key.as_str() else {
            linter_settings.insert(key.clone(), value.clone());
            continue;
        };
        if FORMATTER_NAMES.contains(&name) {
            formatter_settings.insert(key.clone(), value.clone());
            if !formatters_enable.iter().any(|f| f == name) {
                formatters_enable.push(name.to_string());
            }
        } else {
            linter_settings.insert(key.clone(), value.clone());
        }
    }

    let linter = if linter_settings.is_empty() {
        serde_yaml::Value::Null
    } else {
        serde_yaml::Value::Mapping(linter_settings)
    };
    let formatter = if formatter_settings.is_empty() {
        serde_yaml::Value::Null
    } else {
        serde_yaml::Value::Mapping(formatter_settings)
    };
    (linter, formatter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_standard_disable_unused() {
        let yaml = r#"
version: "2"
linters:
  default: standard
  disable:
    - unused
"#;
        let cfg = parse_config_str(yaml).unwrap();
        let sel = cfg.linter_selection();
        let names = sel.resolve_names();
        assert!(!names.contains(&"unused".to_string()));
        assert!(names.contains(&"staticcheck".to_string()));
    }

    #[test]
    fn v2_severity_default_is_spelled_default_not_default_severity() {
        // v1: `severity.default-severity`. v2 renamed it to `severity.default`,
        // and reading only the v1 name left every v2 config's default silently
        // unapplied — the findings kept whatever grade the linter gave them.
        let v2 = parse_config_str(
            "version: \"2\"\nseverity:\n  default: error\n  rules:\n    - severity: info\n      linters: [errcheck]\n",
        )
        .unwrap();
        let sev = v2.effective_severity();
        assert_eq!(sev.default_severity.as_deref(), Some("error"));
        assert_eq!(sev.rules.len(), 1);
        // The v1 spelling is not a v2 key: golangci-lint refuses to load the
        // file rather than honouring it.
        let stray = parse_config_str("version: \"2\"\nseverity:\n  default-severity: error\n").unwrap();
        assert_eq!(stray.effective_severity().default_severity, None);

        let v1 = parse_config_str("severity:\n  default-severity: warning\n").unwrap();
        assert_eq!(
            v1.effective_severity().default_severity.as_deref(),
            Some("warning")
        );
    }

    #[test]
    fn v2_default_all_enables_non_standard_linters() {
        let yaml = r#"
version: "2"
linters:
  default: all
  disable:
    - gosec
"#;
        let cfg = parse_config_str(yaml).unwrap();
        let names = cfg.linter_selection().resolve_names();
        assert!(
            names.contains(&"goconst".to_string()),
            "default: all must enable goconst: {names:?}"
        );
        assert!(
            names.contains(&"revive".to_string()),
            "default: all must enable revive: {names:?}"
        );
        assert!(
            !names.contains(&"gosec".to_string()),
            "explicit disable must still apply: {names:?}"
        );
        assert!(
            names.len() > STANDARD_LINTER_NAMES.len(),
            "default: all must be larger than standard: {}",
            names.len()
        );
    }

    #[test]
    fn v1_enable_all_with_disable() {
        let yaml = r#"
linters:
  enable-all: true
  disable:
    - unused
"#;
        let cfg = parse_config_str(yaml).unwrap();
        assert!(cfg.is_v1());
        let names = cfg.linter_selection().resolve_names();
        assert!(!names.contains(&"unused".to_string()));
        assert!(names.len() >= 4);
    }

    #[test]
    fn v1_disable_all_plus_enable() {
        let yaml = r#"
linters:
  disable-all: true
  enable:
    - govet
    - errcheck
"#;
        let cfg = parse_config_str(yaml).unwrap();
        let names = cfg.linter_selection().resolve_names();
        assert_eq!(names, vec!["govet", "errcheck"]);
    }

    #[test]
    fn normalize_aliases() {
        assert_eq!(normalize_linter_name("gosimple"), "staticcheck");
        assert_eq!(normalize_linter_name("vet"), "govet");
        assert_eq!(normalize_linter_name("structcheck"), "unused");
    }

    #[test]
    fn migrate_v1_enable_all_moves_formatters() {
        let yaml = r#"
linters:
  enable-all: true
  enable:
    - gofmt
    - govet
    - gosimple
"#;
        let cfg = parse_config_str(yaml).unwrap();
        let ConfigFile::V1(v1) = cfg else {
            panic!("expected v1");
        };
        let v2 = migrate_v1_to_v2(&v1);
        assert_eq!(v2.version.as_deref(), Some("2"));
        assert_eq!(v2.linters.default.as_deref(), Some("all"));
        assert!(v2.linters.enable.contains(&"govet".to_string()));
        assert!(v2.linters.enable.contains(&"staticcheck".to_string()));
        assert!(!v2.linters.enable.contains(&"gosimple".to_string()));
        assert!(v2.formatters.enable.contains(&"gofmt".to_string()));
    }

    #[test]
    fn migrate_v1_disable_all_to_none() {
        let yaml = r#"
linters:
  disable-all: true
"#;
        let cfg = parse_config_str(yaml).unwrap();
        let ConfigFile::V1(v1) = cfg else {
            panic!("expected v1");
        };
        let v2 = migrate_v1_to_v2(&v1);
        assert_eq!(v2.linters.default.as_deref(), Some("none"));
    }

    /// A linter written in *both* `enable` and `disable` stays off, whatever
    /// `default` is. k9s's real config does exactly this with `staticcheck`,
    /// and guff used to run it: 9 findings golangci-lint never reports.
    ///
    /// Note the asymmetry with govet's per-analyzer keys, where `enable` is
    /// checked first and therefore wins (`settings.rs::govet_analyzer_enabled`).
    /// The two keys are spelled the same and resolve opposite ways.
    #[test]
    fn linter_in_both_enable_and_disable_is_disabled() {
        for default in [
            LinterDefault::Standard,
            LinterDefault::None,
            LinterDefault::All,
        ] {
            let sel = LinterSelection {
                default,
                enable: vec!["staticcheck".into(), "ineffassign".into()],
                disable: vec!["staticcheck".into()],
            };
            let names = sel.resolve_names();
            assert!(
                !names.contains(&"staticcheck".to_string()),
                "staticcheck should be off for default {default:?}: {names:?}"
            );
            assert!(
                names.contains(&"ineffassign".to_string()),
                "ineffassign should still be on for default {default:?}"
            );
        }
    }

    #[test]
    fn fast_preset_excludes_staticcheck() {
        let sel = LinterSelection {
            default: LinterDefault::Fast,
            ..Default::default()
        };
        let names = sel.resolve_names();
        assert!(!names.contains(&"staticcheck".to_string()));
        assert!(names.contains(&"govet".to_string()));
    }

    #[test]
    fn parse_gofmt_simplify_and_rewrite_rules() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
gofmt:
  simplify: true
  rewrite-rules:
    - pattern: 'interface{}'
      replacement: 'any'
"#,
        )
        .unwrap();
        let opts = parse_gofmt_settings(&yaml);
        assert!(opts.simplify);
        assert_eq!(opts.rewrite_rules.len(), 1);
        assert_eq!(opts.rewrite_rules[0].pattern, "interface{}");
        assert_eq!(opts.rewrite_rules[0].replacement, "any");
    }

    #[test]
    fn parse_gofumpt_extra_rules_and_module_path() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
gofumpt:
  extra-rules: true
  module-path: github.com/org/project
"#,
        )
        .unwrap();
        let opts = parse_gofumpt_settings(&yaml);
        assert!(opts.extra_rules);
        assert_eq!(
            opts.module_path.as_deref(),
            Some("github.com/org/project")
        );
    }

    #[test]
    fn parse_goimports_local_prefixes() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
goimports:
  local-prefixes:
    - github.com/org/project
    - github.com/org/other
"#,
        )
        .unwrap();
        let opts = parse_goimports_settings(&yaml);
        assert_eq!(
            opts.local_prefixes,
            vec![
                "github.com/org/project".to_string(),
                "github.com/org/other".to_string()
            ]
        );
    }

    #[test]
    fn parse_goimports_local_prefixes_comma_string() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
goimports:
  local-prefixes: github.com/a, github.com/b
"#,
        )
        .unwrap();
        let opts = parse_goimports_settings(&yaml);
        assert_eq!(
            opts.local_prefixes,
            vec!["github.com/a".to_string(), "github.com/b".to_string()]
        );
    }

    #[test]
    fn parse_gci_sections_and_flags() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
gci:
  sections:
    - standard
    - default
    - prefix(github.com/org/project)
  custom-order: true
  no-lex-order: true
  no-inline-comments: true
  no-prefix-comments: true
"#,
        )
        .unwrap();
        let opts = parse_gci_settings(&yaml);
        assert_eq!(
            opts.sections,
            vec![
                "standard".to_string(),
                "default".to_string(),
                "prefix(github.com/org/project)".to_string(),
            ]
        );
        assert!(opts.custom_order);
        assert!(opts.no_lex_order);
        assert!(opts.no_inline_comments);
        assert!(opts.no_prefix_comments);
    }

    #[test]
    fn parse_gci_default_when_absent() {
        let yaml: serde_yaml::Value = serde_yaml::from_str("gofmt:\n  simplify: true\n").unwrap();
        let opts = parse_gci_settings(&yaml);
        assert_eq!(
            opts.sections,
            vec!["standard".to_string(), "default".to_string()]
        );
        assert!(!opts.custom_order);
    }

    #[test]
    fn parse_golines_settings_all_keys() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
golines:
  max-len: 120
  tab-len: 8
  shorten-comments: true
  reformat-tags: false
  chain-split-dots: false
"#,
        )
        .unwrap();
        let opts = parse_golines_settings(&yaml);
        assert_eq!(opts.max_len, 120);
        assert_eq!(opts.tab_len, 8);
        assert!(opts.shorten_comments);
        assert!(!opts.reformat_tags);
        assert!(!opts.chain_split_dots);
    }

    #[test]
    fn parse_golines_default_when_absent() {
        let yaml: serde_yaml::Value = serde_yaml::from_str("gofmt:\n  simplify: true\n").unwrap();
        let opts = parse_golines_settings(&yaml);
        assert_eq!(opts, guff_fmt::GolinesOptions::default());
    }

    #[test]
    fn parse_gofumpt_lang_key() {
        let yaml: serde_yaml::Value =
            serde_yaml::from_str("gofumpt:\n  lang: \"1.22\"\n  extra-rules: true\n").unwrap();
        let opts = parse_gofumpt_settings(&yaml);
        assert_eq!(opts.lang.as_deref(), Some("1.22"));
        assert!(opts.extra_rules);
    }

    #[test]
    fn parse_formatters_exclusions_generated_modes() {
        let yaml = r#"
version: "2"
formatters:
  enable:
    - gofmt
  exclusions:
    generated: strict
"#;
        let cfg = parse_config_str(yaml).unwrap();
        let fmt = cfg.formatters();
        assert_eq!(
            fmt.exclusion_generated(),
            guff_fmt::GeneratedMode::Strict
        );

        let yaml_disable = r#"
version: "2"
formatters:
  exclusions:
    generated: disable
"#;
        let fmt = parse_config_str(yaml_disable).unwrap().formatters();
        assert_eq!(
            fmt.exclusion_generated(),
            guff_fmt::GeneratedMode::Disable
        );

        let yaml_default = r#"
version: "2"
formatters:
  enable:
    - gofmt
"#;
        let fmt = parse_config_str(yaml_default).unwrap().formatters();
        assert_eq!(fmt.exclusion_generated(), guff_fmt::GeneratedMode::Lax);
    }

    #[test]
    fn cli_override_default() {
        let sel = LinterSelection::default().with_cli_overrides(Some(LinterDefault::None), &[], &[]);
        assert!(sel.resolve_names().is_empty());
    }

    #[test]
    fn settings_fingerprint_tracks_values_not_just_linter_names() {
        // The issues cache salts on this string. Two configs that enable the
        // same linter with different settings must not collide, or `guff run`
        // replays stale diagnostics after a config edit.
        let a = parse_config_str(
            "version: \"2\"\nlinters:\n  settings:\n    gosec:\n      confidence: low\n",
        )
        .unwrap();
        let b = parse_config_str(
            "version: \"2\"\nlinters:\n  settings:\n    gosec:\n      confidence: medium\n",
        )
        .unwrap();
        assert_ne!(
            a.linter_settings_fingerprint(),
            b.linter_settings_fingerprint()
        );
    }

    #[test]
    fn settings_fingerprint_ignores_yaml_key_order() {
        let a = parse_config_str(
            "version: \"2\"\nlinters:\n  settings:\n    gosec:\n      severity: high\n      confidence: low\n",
        )
        .unwrap();
        let b = parse_config_str(
            "version: \"2\"\nlinters:\n  settings:\n    gosec:\n      confidence: low\n      severity: high\n",
        )
        .unwrap();
        assert_eq!(
            a.linter_settings_fingerprint(),
            b.linter_settings_fingerprint()
        );
    }

    #[test]
    fn canonical_yaml_sorts_nested_mappings() {
        let v: serde_yaml::Value =
            serde_yaml::from_str("b: 1\na:\n  z: [3, 1]\n  y: true\n").unwrap();
        assert_eq!(
            canonical_yaml(&v),
            "{\"a\":{\"y\":true,\"z\":[3,1]},\"b\":1}"
        );
    }
}
