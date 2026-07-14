//! Configuration file parsing and golangci-lint compatibility.
//!
//! Supports reading `.golangci.yml` / `.guff.yml` (v1 and v2) and resolving
//! enabled linters the same way as the CLI `--preset` / `--enable` / `--disable`.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::registry::{FAST_LINTER_NAMES, STANDARD_LINTER_NAMES};

/// Default config file names searched from the working directory upward.
pub const CONFIG_FILE_NAMES: &[&str] = &[
    ".golangci.yml",
    ".golangci.yaml",
    ".guff.yml",
    ".guff.yaml",
];

/// Linter names moved to the `formatters` section in golangci-lint v2.
pub const FORMATTER_NAMES: &[&str] = &["gci", "gofmt", "gofumpt", "goimports"];

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
            Self::Standard | Self::All => STANDARD_LINTER_NAMES,
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
        let mut names: Vec<String> = self
            .default
            .base_linters()
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let disable: HashSet<&str> = self.disable.iter().map(|s| s.as_str()).collect();
        names.retain(|n| !disable.contains(n.as_str()));

        for e in &self.enable {
            let normalized = normalize_linter_name(e);
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
    /// DEFERRED: diff-based filtering (→ R2 follow-up / processor port).
    #[serde(default)]
    pub new: bool,
    #[serde(default, rename = "new-from-rev")]
    pub new_from_rev: Option<String>,
    #[serde(default, rename = "new-from-merge-base")]
    pub new_from_merge_base: Option<String>,
    #[serde(default, rename = "new-from-patch")]
    pub new_from_patch: Option<String>,
    #[serde(default)]
    pub include: Vec<String>,
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
            include: Vec::new(),
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
            && self.include.is_empty()
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
    #[serde(default, rename = "default-severity")]
    pub default_severity: Option<String>,
    #[serde(default, rename = "case-sensitive")]
    pub case_sensitive: bool,
    #[serde(default)]
    pub rules: Vec<SeverityRule>,
}

impl SeverityConfig {
    fn is_default(&self) -> bool {
        self.default_severity.is_none() && !self.case_sensitive && self.rules.is_empty()
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
    #[serde(default, skip_serializing_if = "serde_yaml::Value::is_null")]
    pub settings: serde_yaml::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FormattersV2 {
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default, skip_serializing_if = "serde_yaml::Value::is_null")]
    pub settings: serde_yaml::Value,
}

impl FormattersV2 {
    fn is_empty(&self) -> bool {
        self.enable.is_empty() && self.settings.is_null()
    }
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

    pub fn output(&self) -> &OutputConfig {
        match self {
            Self::V1(v1) => &v1.output,
            Self::V2(v2) => &v2.output,
        }
    }

    /// Raw `linters.settings` (v2) or `linters-settings` (v1) YAML.
    pub fn linter_settings_raw(&self) -> &serde_yaml::Value {
        match self {
            Self::V1(v1) => &v1.linters_settings,
            Self::V2(v2) => &v2.linters.settings,
        }
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
const COMMENT_PRESET: &[&str] = &["dupword", "godot", "godox", "misspell"];
const COMPLEXITY_PRESET: &[&str] =
    &["cyclop", "funlen", "gocognit", "gocyclo", "maintidx", "nestif"];
const ERROR_PRESET: &[&str] = &["err113", "errcheck", "errorlint", "wrapcheck"];
const FORMAT_PRESET: &[&str] = &["gci", "gofmt", "gofumpt", "goimports"];
const IMPORT_PRESET: &[&str] = &["depguard", "gci", "goimports", "gomodguard"];
const METALINTER_PRESET: &[&str] = &["gocritic", "govet", "revive", "staticcheck"];
const MODULE_PRESET: &[&str] = &["depguard", "gomoddirectives", "gomodguard"];
const PERFORMANCE_PRESET: &[&str] = &["bodyclose", "fatcontext", "noctx", "perfsprint", "prealloc"];
const SQL_PRESET: &[&str] = &["rowserrcheck", "sqlclosecheck"];
const STYLE_PRESET: &[&str] = &[
    "asciicheck", "canonicalheader", "containedctx", "copyloopvar", "decorder", "depguard",
    "dogsled", "dupl", "err113", "errname", "exhaustruct", "exptostd", "forbidigo",
    "forcetypeassert", "ginkgolinter", "gochecknoglobals", "gochecknoinits", "goconst",
    "gocritic", "godot", "godox", "goheader", "gomoddirectives", "gomodguard",
    "goprintffuncname", "gosimple", "grouper", "iface", "importas", "inamedparam",
    "interfacebloat", "intrange", "ireturn", "lll", "loggercheck", "makezero", "mirror",
    "misspell", "mnd", "musttag", "nakedret", "nilnil", "nlreturn", "nolintlint",
    "nonamedreturns", "nosprintfhostport", "paralleltest", "predeclared", "promlinter",
    "revive", "sloglint", "stylecheck", "tagalign", "tagliatelle", "testpackage",
    "tparallel", "unconvert", "usestdlibvars", "varnamelen", "wastedassign", "whitespace",
    "wrapcheck", "wsl",
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
            settings: linter_settings,
        },
        formatters: FormattersV2 {
            enable: dedupe_normalized(formatters_enable),
            settings: formatter_settings,
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
    fn cli_override_default() {
        let sel = LinterSelection::default().with_cli_overrides(Some(LinterDefault::None), &[], &[]);
        assert!(sel.resolve_names().is_empty());
    }
}
