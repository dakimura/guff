//! Typed `linters.settings` (golangci-lint `linters-settings` / v2 `linters.settings`).
//!
//! Parses YAML into per-linter structs, builds a [`SettingsBag`] for Pass-time
//! options (errcheck), and filters analyzer lists for selection-time options
//! (govet enable/disable, staticcheck checks).

use std::collections::HashSet;
use std::sync::Arc;

use guff_analysis::{Analyzer, SettingsBag};
use serde::Deserialize;

/// All per-linter settings currently understood by guff.
#[derive(Debug, Clone, Default)]
pub struct LinterSettings {
    pub errcheck: ErrcheckSettings,
    pub govet: GovetSettings,
    pub staticcheck: StaticcheckSettings,
    pub revive: ReviveSettings,
    pub dupl: DuplSettings,
    pub misspell: MisspellSettings,
    pub gocyclo: GocycloSettings,
    pub maintidx: MaintidxSettings,
    pub gocognit: GocognitSettings,
    pub nestif: NestifSettings,
    pub dogsled: DogsledSettings,
    pub funlen: FunlenSettings,
    pub cyclop: CyclopSettings,
    pub lll: LllSettings,
    pub nakedret: NakedretSettings,
    pub nlreturn: NlreturnSettings,
    pub predeclared: PredeclaredSettings,
    pub whitespace: WhitespaceSettings,
    pub mnd: MndSettings,
    pub prealloc: PreallocSettings,
    pub tagalign: TagalignSettings,
    pub wsl: WslSettings,
    pub perfsprint: PerfsprintSettings,
    pub goconst: GoconstSettings,
    pub copyloopvar: CopyloopvarSettings,
    pub usetesting: UsetestingSettings,
    pub usestdlibvars: UsestdlibvarsSettings,
    pub unconvert: UnconvertSettings,
    pub exhaustruct: ExhaustructSettings,
    pub exhaustive: ExhaustiveSettings,
    pub musttag: MusttagSettings,
    pub loggercheck: LoggercheckSettings,
    pub sloglint: SloglintSettings,
    pub testifylint: TestifylintSettings,
    pub errchkjson: ErrchkjsonSettings,
    pub wrapcheck: WrapcheckSettings,
    pub rowserrcheck: RowserrcheckSettings,
    pub bodyclose: BodycloseSettings,
    pub godot: GodotSettings,
    pub godox: GodoxSettings,
    pub dupword: DupwordSettings,
    pub godoclint: GodoclintSettings,
    pub depguard: DepguardSettings,
    pub gomoddirectives: GomoddirectivesSettings,
    pub gomodguard: GomodguardSettings,
    pub modernize: ModernizeSettings,
    pub gocritic: GocriticSettings,
    pub forbidigo: ForbidigoSettings,
    pub bidichk: BidichkSettings,
    pub gosmopolitan: GosmopolitanSettings,
    pub goheader: GoheaderSettings,
    pub asasalint: AsasalintSettings,
    pub importas: ImportasSettings,
    pub reassign: ReassignSettings,
    pub recvcheck: RecvcheckSettings,
    pub thelper: ThelperSettings,
    pub iface: IfaceSettings,
    pub interfacebloat: InterfacebloatSettings,
    pub embeddedstructfieldcheck: EmbeddedstructfieldcheckSettings,
    pub gochecksumtype: GochecksumtypeSettings,
    pub inamedparam: InamedparamSettings,
    pub nonamedreturns: NonamedreturnsSettings,
    pub paralleltest: ParalleltestSettings,
    pub testpackage: TestpackageSettings,
    pub tagliatelle: TagliatelleSettings,
    pub decorder: DecorderSettings,
    pub iotamixing: IotamixingSettings,
    pub grouper: GrouperSettings,
    pub ireturn: IreturnSettings,
    pub gosec: GosecSettings,
    pub nolintlint: NolintlintSettings,
    pub funcorder: FuncorderSettings,
    pub varnamelen: VarnamelenSettings,
    pub unparam: UnparamSettings,
    pub unqueryvet: UnqueryvetSettings,
    pub promlinter: PromlinterSettings,
    pub ginkgolinter: GinkgolinterSettings,
    pub wsl_v5: WslV5Settings,
    /// golangci `linters.settings.custom` (module plugins).
    pub custom: std::collections::HashMap<String, CustomLinterConfig>,
}

/// One entry under `linters.settings.custom.<name>`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct CustomLinterConfig {
    /// Must be `"module"` for guff module plugins (golangci also has `"goplugin"`).
    #[serde(default, rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub description: String,
    /// Nested settings passed to the plugin's `New` factory.
    #[serde(default)]
    pub settings: serde_yaml::Value,
    /// golangci goplugin path (unsupported; accepted for forward-compat).
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default, rename = "original-url")]
    pub original_url: Option<String>,
}

/// `linters.settings.errcheck` / `linters-settings.errcheck`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ErrcheckSettings {
    /// Report `_ = f()` / `x, _ := f()` when the ignored value is an error.
    #[serde(default, rename = "check-blank")]
    pub check_blank: bool,
    /// Report ignored type assertions `x := y.(T)` (no ok).
    #[serde(default, rename = "check-type-assertions")]
    pub check_type_assertions: bool,
    /// Skip the built-in default exclusion list (kisielk `-excludeonly`).
    #[serde(default, rename = "disable-default-exclusions")]
    pub disable_default_exclusions: bool,
    /// Additional function/method symbols to exclude (kisielk exclude file format).
    #[serde(default, rename = "exclude-functions", deserialize_with = "string_or_seq")]
    pub exclude_functions: Vec<String>,
    // DEFERRED (R4 follow-up): verbose.
}

/// `linters.settings.govet` / `linters-settings.govet`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GovetSettings {
    #[serde(default, rename = "enable-all")]
    pub enable_all: bool,
    #[serde(default, rename = "disable-all")]
    pub disable_all: bool,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub enable: Vec<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub disable: Vec<String>,
}

/// `linters.settings.staticcheck` / `stylecheck` / `linters-settings.*`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct StaticcheckSettings {
    /// Check selectors (`all`, `SA1000`, `-SA1000`, …). `None` = keep registry default.
    #[serde(default)]
    pub checks: Option<Vec<String>>,
    /// ST1003 known initialisms (`initialisms`). Empty/`None` → upstream defaults.
    #[serde(default)]
    pub initialisms: Option<Vec<String>>,
    /// ST1001 packages allowed as dot imports (`dot-import-whitelist`).
    #[serde(default, rename = "dot-import-whitelist")]
    pub dot_import_whitelist: Option<Vec<String>>,
    /// ST1013 numeric codes that are not reported (`http-status-code-whitelist`).
    #[serde(default, rename = "http-status-code-whitelist")]
    pub http_status_code_whitelist: Option<Vec<String>>,
}

impl StaticcheckSettings {
    /// Merge another block (typically `stylecheck`) over this one.
    /// Non-`None` fields in `other` win.
    pub fn merge_stylecheck(&mut self, other: StaticcheckSettings) {
        if other.checks.is_some() {
            self.checks = other.checks;
        }
        if other.initialisms.is_some() {
            self.initialisms = other.initialisms;
        }
        if other.dot_import_whitelist.is_some() {
            self.dot_import_whitelist = other.dot_import_whitelist;
        }
        if other.http_status_code_whitelist.is_some() {
            self.http_status_code_whitelist = other.http_status_code_whitelist;
        }
    }

    pub fn to_guff_stylecheck(&self) -> guff_staticcheck::StylecheckOptions {
        guff_staticcheck::StylecheckOptions {
            initialisms: self.initialisms.clone(),
            dot_import_whitelist: self.dot_import_whitelist.clone(),
            http_status_code_whitelist: self.http_status_code_whitelist.clone(),
        }
    }
}

/// `linters.settings.revive` / `linters-settings.revive`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReviveSettings {
    /// Default severity for revive failures (`warning`, `error`, …).
    #[serde(default)]
    pub severity: Option<String>,
    /// Per-rule enablement and arguments. `None` = golint-default rules only.
    #[serde(default)]
    pub rules: Option<Vec<ReviveRuleSetting>>,
    /// Minimum failure confidence to report (revive default: 0.8).
    #[serde(default)]
    pub confidence: Option<f64>,
    /// When true, skip diagnostics in generated files.
    #[serde(default, rename = "ignore-generated-header")]
    pub ignore_generated_header: bool,
    /// Merge golint-default rules when a `rules:` list is present (golangci).
    #[serde(default, rename = "enable-default-rules")]
    pub enable_default_rules: bool,
    /// Enable every known revive rule (golangci).
    #[serde(default, rename = "enable-all-rules")]
    pub enable_all_rules: bool,
}

/// One revive rule entry from golangci-lint YAML.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ReviveRuleSetting {
    pub name: String,
    #[serde(default)]
    pub arguments: Vec<serde_yaml::Value>,
    #[serde(default)]
    pub disabled: bool,
    /// Per-rule severity override (`warning`, `error`, …).
    #[serde(default)]
    pub severity: Option<String>,
    // DEFERRED: exclude.
}

/// `linters.settings.dupl` / `linters-settings.dupl`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DuplSettings {
    /// Token-count threshold for clone detection (golangci default: 150).
    #[serde(default)]
    pub threshold: Option<i32>,
}

/// `linters.settings.misspell` / `linters-settings.misspell`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct MisspellSettings {
    #[serde(default)]
    pub locale: Option<String>,
    /// golangci v1 `ignore-words`; v2 renamed to `ignore-rules` (both accepted).
    #[serde(default, rename = "ignore-words", alias = "ignore-rules", deserialize_with = "string_or_seq")]
    pub ignore_words: Vec<String>,
    #[serde(default, rename = "extra-words")]
    pub extra_words: Vec<MisspellExtraWordSetting>,
    #[serde(default)]
    pub mode: Option<String>,
}

/// One `extra-words` entry from golangci-lint YAML.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct MisspellExtraWordSetting {
    pub typo: String,
    pub correction: String,
}

/// `linters.settings.gocyclo` / `linters-settings.gocyclo`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GocycloSettings {
    #[serde(default, rename = "min-complexity")]
    pub min_complexity: Option<usize>,
}

/// `linters.settings.maintidx` / `linters-settings.maintidx`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MaintidxSettings {
    /// Report functions with maintainability index < N (upstream default 20).
    #[serde(default = "default_maintidx_under")]
    pub under: usize,
}

fn default_maintidx_under() -> usize {
    20
}

impl Default for MaintidxSettings {
    fn default() -> Self {
        Self {
            under: default_maintidx_under(),
        }
    }
}

impl MaintidxSettings {
    pub fn to_guff_maintidx(&self) -> guff_style::MaintidxOptions {
        guff_style::MaintidxOptions { under: self.under }
    }
}

/// `linters.settings.gocognit` / `linters-settings.gocognit`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GocognitSettings {
    #[serde(default, rename = "min-complexity")]
    pub min_complexity: Option<usize>,
}

/// `linters.settings.nestif` / `linters-settings.nestif`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct NestifSettings {
    #[serde(default, rename = "min-complexity")]
    pub min_complexity: Option<usize>,
}

/// `linters.settings.dogsled` / `linters-settings.dogsled`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DogsledSettings {
    #[serde(default, rename = "max-blank-identifiers")]
    pub max_blank_identifiers: Option<usize>,
}

/// `linters.settings.funlen` / `linters-settings.funlen`.
///
/// golangci accepts negative `lines` / `statements` (commonly `-1`) to disable
/// that half of the check. Deserialize as `i64` so a lone `-1` does not fail
/// the whole settings object (which previously silently fell back to defaults).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct FunlenSettings {
    #[serde(default)]
    pub lines: Option<i64>,
    #[serde(default)]
    pub statements: Option<i64>,
    #[serde(default, rename = "ignore-comments")]
    pub ignore_comments: Option<bool>,
}

/// `linters.settings.cyclop` / `linters-settings.cyclop`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct CyclopSettings {
    #[serde(default, rename = "max-complexity")]
    pub max_complexity: Option<usize>,
    #[serde(default, rename = "package-average")]
    pub package_average: Option<f64>,
    #[serde(default, rename = "skip-tests")]
    pub skip_tests: Option<bool>,
}

/// `linters.settings.lll` / `linters-settings.lll`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct LllSettings {
    #[serde(default, rename = "line-length")]
    pub line_length: Option<usize>,
    #[serde(default, rename = "tab-width")]
    pub tab_width: Option<usize>,
}

/// `linters.settings.nakedret` / `linters-settings.nakedret`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct NakedretSettings {
    #[serde(default, rename = "max-func-lines")]
    pub max_func_lines: Option<usize>,
    #[serde(default, rename = "skip-test-files")]
    pub skip_test_files: Option<bool>,
}

/// `linters.settings.nlreturn` / `linters-settings.nlreturn`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct NlreturnSettings {
    #[serde(default, rename = "block-size")]
    pub block_size: Option<i64>,
}

/// `linters.settings.predeclared` / `linters-settings.predeclared`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct PredeclaredSettings {
    #[serde(default, deserialize_with = "string_or_seq")]
    pub ignore: Vec<String>,
    #[serde(default, rename = "q", alias = "qualified-name")]
    pub qualified: Option<bool>,
}

/// `linters.settings.whitespace` / `linters-settings.whitespace`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct WhitespaceSettings {
    #[serde(default, rename = "multi-if")]
    pub multi_if: Option<bool>,
    #[serde(default, rename = "multi-func")]
    pub multi_func: Option<bool>,
}

/// `linters.settings.mnd` / `linters-settings.mnd`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct MndSettings {
    #[serde(default)]
    pub checks: Option<Vec<String>>,
    #[serde(default, rename = "ignored-numbers", deserialize_with = "string_or_seq")]
    pub ignored_numbers: Vec<String>,
    #[serde(default, rename = "ignored-files", deserialize_with = "string_or_seq")]
    pub ignored_files: Vec<String>,
    #[serde(default, rename = "ignored-functions", deserialize_with = "string_or_seq")]
    pub ignored_functions: Vec<String>,
}

/// `linters.settings.prealloc` / `linters-settings.prealloc`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct PreallocSettings {
    #[serde(default)]
    pub simple: Option<bool>,
    #[serde(default, rename = "range-loops")]
    pub range_loops: Option<bool>,
    #[serde(default, rename = "for-loops")]
    pub for_loops: Option<bool>,
}

/// `linters.settings.tagalign` / `linters-settings.tagalign`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TagalignSettings {
    #[serde(default)]
    pub align: Option<bool>,
    #[serde(default)]
    pub sort: Option<bool>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub order: Vec<String>,
    #[serde(default)]
    pub strict: Option<bool>,
}

/// `linters.settings.wsl` / `linters-settings.wsl`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct WslSettings {
    #[serde(default, rename = "strict-append")]
    pub strict_append: Option<bool>,
    #[serde(default, rename = "allow-assign-and-call")]
    pub allow_assign_and_call: Option<bool>,
    #[serde(default, rename = "allow-assign-and-anything")]
    pub allow_assign_and_anything: Option<bool>,
    #[serde(default, rename = "allow-multiline-assign")]
    pub allow_multiline_assign: Option<bool>,
    #[serde(default, rename = "allow-cuddle-with-calls", deserialize_with = "string_or_seq")]
    pub allow_cuddle_with_calls: Vec<String>,
    #[serde(default, rename = "allow-cuddle-with-rhs", deserialize_with = "string_or_seq")]
    pub allow_cuddle_with_rhs: Vec<String>,
}

/// `linters.settings.perfsprint` / `linters-settings.perfsprint`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct PerfsprintSettings {
    #[serde(default, rename = "integer-format")]
    pub integer_format: Option<bool>,
    #[serde(default, rename = "int-conversion")]
    pub int_conversion: Option<bool>,
    #[serde(default, rename = "error-format")]
    pub error_format: Option<bool>,
    #[serde(default, rename = "err-error")]
    pub err_error: Option<bool>,
    #[serde(default, rename = "errorf")]
    pub errorf: Option<bool>,
    #[serde(default, rename = "string-format")]
    pub string_format: Option<bool>,
    #[serde(default, rename = "sprintf1")]
    pub sprintf1: Option<bool>,
    #[serde(default, rename = "strconcat")]
    pub strconcat: Option<bool>,
    #[serde(default, rename = "bool-format")]
    pub bool_format: Option<bool>,
    #[serde(default, rename = "hex-format")]
    pub hex_format: Option<bool>,
    #[serde(default, rename = "concat-loop")]
    pub concat_loop: Option<bool>,
    #[serde(default, rename = "loop-other-ops")]
    pub loop_other_ops: Option<bool>,
}

/// `linters.settings.goconst` / `linters-settings.goconst`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GoconstSettings {
    #[serde(default, rename = "min-len")]
    pub min_len: Option<usize>,
    #[serde(default, rename = "min-occurrences")]
    pub min_occurrences: Option<usize>,
    #[serde(default, rename = "ignore-calls")]
    pub ignore_calls: Option<bool>,
    #[serde(default, rename = "ignore-tests")]
    pub ignore_tests: Option<bool>,
    #[serde(default, rename = "match-constant")]
    pub match_constant: Option<bool>,
    #[serde(default, rename = "find-duplicates")]
    pub find_duplicates: Option<bool>,
    pub numbers: Option<bool>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    /// Regex patterns for strings to ignore (golangci `ignore-string-values`).
    #[serde(default, rename = "ignore-string-values", deserialize_with = "string_or_seq")]
    pub ignore_string_values: Vec<String>,
    /// Deprecated single-pattern form; merged into [`Self::ignore_string_values`].
    #[serde(default, rename = "ignore-strings")]
    pub ignore_strings: Option<String>,
}

/// `linters.settings.copyloopvar` / `linters-settings.copyloopvar`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct CopyloopvarSettings {
    #[serde(default, rename = "check-alias")]
    pub check_alias: Option<bool>,
}

/// `linters.settings.unconvert` / `linters-settings.unconvert`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct UnconvertSettings {
    #[serde(default, rename = "fast-math")]
    pub fast_math: Option<bool>,
    #[serde(default)]
    pub safe: Option<bool>,
}

/// `linters.settings.exhaustruct` / `linters-settings.exhaustruct`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ExhaustructSettings {
    #[serde(default, deserialize_with = "string_or_seq")]
    pub include: Vec<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub exclude: Vec<String>,
    #[serde(default, rename = "allow-empty")]
    pub allow_empty: Option<bool>,
    #[serde(default, rename = "allow-empty-rx", deserialize_with = "string_or_seq")]
    pub allow_empty_rx: Vec<String>,
    #[serde(default, rename = "allow-empty-returns")]
    pub allow_empty_returns: Option<bool>,
    #[serde(default, rename = "allow-empty-declarations")]
    pub allow_empty_declarations: Option<bool>,
}

/// `linters.settings.exhaustive` / `linters-settings.exhaustive`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ExhaustiveSettings {
    /// Program elements to check: `switch` and/or `map` (default `[switch]`).
    #[serde(default, deserialize_with = "string_or_seq")]
    pub check: Vec<String>,
    #[serde(default, rename = "default-signifies-exhaustive")]
    pub default_signifies_exhaustive: Option<bool>,
    #[serde(default, rename = "default-case-required")]
    pub default_case_required: Option<bool>,
    #[serde(default, rename = "ignore-enum-members")]
    pub ignore_enum_members: Option<String>,
    #[serde(default, rename = "ignore-enum-types")]
    pub ignore_enum_types: Option<String>,
    #[serde(default, rename = "package-scope-only")]
    pub package_scope_only: Option<bool>,
    // DEFERRED: explicit-exhaustive-switch / explicit-exhaustive-map / check-generated.
}

/// One entry in `linters.settings.musttag.functions`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct MusttagFuncSettings {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default, rename = "arg-pos")]
    pub arg_pos: usize,
}

/// `linters.settings.musttag` / `linters-settings.musttag`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct MusttagSettings {
    #[serde(default)]
    pub functions: Vec<MusttagFuncSettings>,
}

/// `linters.settings.loggercheck` / `linters-settings.loggercheck`.
///
/// Checker keys default to enabled when omitted (golangci-lint defaults).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct LoggercheckSettings {
    #[serde(default)]
    pub kitlog: Option<bool>,
    #[serde(default)]
    pub klog: Option<bool>,
    #[serde(default)]
    pub logr: Option<bool>,
    #[serde(default)]
    pub slog: Option<bool>,
    #[serde(default)]
    pub zap: Option<bool>,
    #[serde(default, rename = "require-string-key")]
    pub require_string_key: bool,
    #[serde(default, rename = "no-printf-like")]
    pub no_printf_like: bool,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub rules: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// Custom function entry for `linters.settings.sloglint.custom-funcs`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct SloglintFuncSettings {
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "msg-pos")]
    pub msg_pos: i32,
    #[serde(default, rename = "args-pos")]
    pub args_pos: i32,
}

/// `linters.settings.sloglint` / `linters-settings.sloglint`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SloglintSettings {
    #[serde(default = "default_true", rename = "no-mixed-args")]
    pub no_mixed_args: bool,
    #[serde(default, rename = "kv-only")]
    pub kv_only: bool,
    #[serde(default, rename = "attr-only")]
    pub attr_only: bool,
    #[serde(default, rename = "no-global")]
    pub no_global: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default, rename = "static-msg")]
    pub static_msg: bool,
    #[serde(default, rename = "msg-style")]
    pub msg_style: Option<String>,
    #[serde(default, rename = "no-raw-keys")]
    pub no_raw_keys: bool,
    #[serde(default, rename = "key-naming-case")]
    pub key_naming_case: Option<String>,
    #[serde(default, rename = "allowed-keys", deserialize_with = "string_or_seq")]
    pub allowed_keys: Vec<String>,
    #[serde(default, rename = "forbidden-keys", deserialize_with = "string_or_seq")]
    pub forbidden_keys: Vec<String>,
    #[serde(default, rename = "args-on-sep-lines")]
    pub args_on_sep_lines: bool,
    #[serde(default, rename = "custom-funcs")]
    pub custom_funcs: Vec<SloglintFuncSettings>,
}

impl Default for SloglintSettings {
    fn default() -> Self {
        Self {
            no_mixed_args: true,
            kv_only: false,
            attr_only: false,
            no_global: None,
            context: None,
            static_msg: false,
            msg_style: None,
            no_raw_keys: false,
            key_naming_case: None,
            allowed_keys: Vec::new(),
            forbidden_keys: Vec::new(),
            args_on_sep_lines: false,
            custom_funcs: Vec::new(),
        }
    }
}

/// Nested `bool-compare` settings for testifylint.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TestifylintBoolCompareSettings {
    #[serde(default, rename = "ignore-custom-types")]
    pub ignore_custom_types: bool,
}

/// Nested `expected-actual` settings for testifylint.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TestifylintExpectedActualSettings {
    #[serde(default)]
    pub pattern: Option<String>,
}

/// Nested `time-compare` settings for testifylint.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TestifylintTimeCompareSettings {
    #[serde(default, rename = "suppress-calls-pattern")]
    pub suppress_calls_pattern: Option<String>,
}

/// Nested `formatter` settings for testifylint.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TestifylintFormatterSettings {
    #[serde(default = "default_true", rename = "check-format-string")]
    pub check_format_string: bool,
    #[serde(default, rename = "require-f-funcs")]
    pub require_f_funcs: bool,
    // golangci-lint always injects this flag into the analyzer config; the Go
    // zero value is false, which overrides testifylint's own default (true).
    // Match that effective default so unset YAML agrees with golangci 2.12.
    #[serde(default, rename = "require-string-msg")]
    pub require_string_msg: bool,
}

/// Nested `suite-extra-assert-call` settings for testifylint.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TestifylintSuiteExtraAssertCallSettings {
    /// `"remove"` (default) or `"require"`.
    #[serde(default)]
    pub mode: Option<String>,
}

/// Nested `require-error` settings for testifylint.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TestifylintRequireErrorSettings {
    /// Regex matching assertion function names (e.g. `^NoError(f)?$`).
    #[serde(default, rename = "fn-pattern")]
    pub fn_pattern: Option<String>,
}

/// Nested `go-require` settings for testifylint.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TestifylintGoRequireSettings {
    #[serde(default, rename = "ignore-http-handlers")]
    pub ignore_http_handlers: bool,
}

impl Default for TestifylintFormatterSettings {
    fn default() -> Self {
        Self {
            check_format_string: true,
            require_f_funcs: false,
            // Match golangci-lint's injected zero-value when YAML omits the key.
            require_string_msg: false,
        }
    }
}

/// `linters.settings.testifylint` / `linters-settings.testifylint`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TestifylintSettings {
    #[serde(default, rename = "enable-all")]
    pub enable_all: bool,
    #[serde(default, rename = "disable-all")]
    pub disable_all: bool,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub enable: Vec<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub disable: Vec<String>,
    #[serde(default, rename = "bool-compare")]
    pub bool_compare: TestifylintBoolCompareSettings,
    #[serde(default, rename = "expected-actual")]
    pub expected_actual: TestifylintExpectedActualSettings,
    #[serde(default, rename = "time-compare")]
    pub time_compare: TestifylintTimeCompareSettings,
    #[serde(default)]
    pub formatter: TestifylintFormatterSettings,
    #[serde(default, rename = "suite-extra-assert-call")]
    pub suite_extra_assert_call: TestifylintSuiteExtraAssertCallSettings,
    #[serde(default, rename = "require-error")]
    pub require_error: TestifylintRequireErrorSettings,
    #[serde(default, rename = "go-require")]
    pub go_require: TestifylintGoRequireSettings,
}

/// `linters.settings.usetesting` / `linters-settings.usetesting`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct UsetestingSettings {
    #[serde(default, rename = "os-create-temp")]
    pub os_create_temp: Option<bool>,
    #[serde(default, rename = "os-mkdir-temp")]
    pub os_mkdir_temp: Option<bool>,
    #[serde(default, rename = "os-setenv")]
    pub os_setenv: Option<bool>,
    #[serde(default, rename = "os-temp-dir")]
    pub os_temp_dir: Option<bool>,
    #[serde(default, rename = "os-chdir")]
    pub os_chdir: Option<bool>,
    #[serde(default, rename = "context-background")]
    pub context_background: Option<bool>,
    #[serde(default, rename = "context-todo")]
    pub context_todo: Option<bool>,
}

/// `linters.settings.usestdlibvars` / `linters-settings.usestdlibvars`.
///
/// HTTP checks default on; optional tables default off (golangci / upstream).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct UsestdlibvarsSettings {
    #[serde(default, rename = "http-method")]
    pub http_method: Option<bool>,
    #[serde(default, rename = "http-status-code")]
    pub http_status_code: Option<bool>,
    #[serde(default, rename = "time-weekday")]
    pub time_weekday: Option<bool>,
    #[serde(default, rename = "time-month")]
    pub time_month: Option<bool>,
    #[serde(default, rename = "time-layout")]
    pub time_layout: Option<bool>,
    #[serde(default, rename = "crypto-hash")]
    pub crypto_hash: Option<bool>,
    #[serde(default, rename = "default-rpc-path")]
    pub default_rpc_path: Option<bool>,
    #[serde(default, rename = "sql-isolation-level")]
    pub sql_isolation_level: Option<bool>,
    #[serde(default, rename = "tls-signature-scheme")]
    pub tls_signature_scheme: Option<bool>,
    #[serde(default, rename = "constant-kind")]
    pub constant_kind: Option<bool>,
    #[serde(default, rename = "time-date-month")]
    pub time_date_month: Option<bool>,
}

/// `linters.settings.errchkjson` / `linters-settings.errchkjson`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ErrchkjsonSettings {
    /// When true, report checked errors on safe encodings (`omit-safe: false`).
    /// golangci-lint default: false.
    #[serde(default, rename = "check-error-free-encoding")]
    pub check_error_free_encoding: bool,
    /// When true, report structs with no exported JSON fields.
    /// golangci-lint default: false.
    #[serde(default, rename = "report-no-exported")]
    pub report_no_exported: bool,
}

/// `linters.settings.wrapcheck` / `linters-settings.wrapcheck`.
///
/// `None` for `ignore-sigs` keeps upstream default ignore substrings.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct WrapcheckSettings {
    #[serde(default, rename = "ignore-sigs")]
    pub ignore_sigs: Option<Vec<String>>,
    #[serde(default, rename = "extra-ignore-sigs", deserialize_with = "string_or_seq")]
    pub extra_ignore_sigs: Vec<String>,
    #[serde(default, rename = "ignore-sig-regexps", deserialize_with = "string_or_seq")]
    pub ignore_sig_regexps: Vec<String>,
    #[serde(default, rename = "ignore-package-globs", deserialize_with = "string_or_seq")]
    pub ignore_package_globs: Vec<String>,
    #[serde(default, rename = "ignore-interface-regexps", deserialize_with = "string_or_seq")]
    pub ignore_interface_regexps: Vec<String>,
    #[serde(default, rename = "report-internal-errors")]
    pub report_internal_errors: bool,
}

/// `linters.settings.rowserrcheck` / `linters-settings.rowserrcheck`.
///
/// `database/sql` is always checked; `packages` lists additional import paths.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct RowserrcheckSettings {
    #[serde(default, deserialize_with = "string_or_seq")]
    pub packages: Vec<String>,
}

/// `linters.settings.bodyclose` / `linters-settings.bodyclose`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct BodycloseSettings {
    /// Also require a known consumption call on the response body.
    /// Default: false (golangci-lint parity).
    #[serde(default, rename = "check-consumption")]
    pub check_consumption: bool,
}

/// `linters.settings.godot` / `linters-settings.godot`.
///
/// `toplevel` / `noinline` scopes remain DEFERRED (fall back to declarations).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GodotSettings {
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub period: Option<bool>,
    #[serde(default)]
    pub capital: Option<bool>,
}

/// `linters.settings.godox` / `linters-settings.godox`.
///
/// Empty `keywords` → golangci defaults (`TODO` / `BUG` / `FIXME`).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GodoxSettings {
    #[serde(default, deserialize_with = "string_or_seq")]
    pub keywords: Vec<String>,
}

/// `linters.settings.dupword` / `linters-settings.dupword`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DupwordSettings {
    #[serde(default, deserialize_with = "string_or_seq")]
    pub keywords: Vec<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub ignore: Vec<String>,
    #[serde(default, rename = "comments-only")]
    pub comments_only: Option<bool>,
}

/// `linters.settings.godoclint` / `linters-settings.godoclint`.
///
/// `default` is `basic` | `all` | `none` (golangci default: `basic`).
/// Per-rule `options` are DEFERRED.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GodoclintSettings {
    #[serde(default, rename = "default")]
    pub default_set: Option<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub enable: Vec<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub disable: Vec<String>,
}

/// `linters.settings.modernize` / `linters-settings.modernize`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ModernizeSettings {
    /// Checker names to disable (golangci-lint compatible).
    #[serde(default, deserialize_with = "string_or_seq")]
    pub disable: Vec<String>,
}

/// `linters.settings.gocritic` / `linters-settings.gocritic`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GocriticSettings {
    #[serde(default, rename = "enable-all")]
    pub enable_all: bool,
    #[serde(default, rename = "disable-all")]
    pub disable_all: bool,
    #[serde(default, rename = "enabled-checks", deserialize_with = "string_or_seq")]
    pub enabled_checks: Vec<String>,
    #[serde(default, rename = "disabled-checks", deserialize_with = "string_or_seq")]
    pub disabled_checks: Vec<String>,
    #[serde(default, rename = "enabled-tags", deserialize_with = "string_or_seq")]
    pub enabled_tags: Vec<String>,
    #[serde(default, rename = "disabled-tags", deserialize_with = "string_or_seq")]
    pub disabled_tags: Vec<String>,
    /// Per-check parameters (`gocritic.settings.<check>.<param>`). Kept raw
    /// because each check declares its own param names and types.
    #[serde(default)]
    pub settings: Option<serde_yaml::Value>,
}

/// One `forbidigo.forbid` entry (string or `{pattern,msg,pkg}`).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ForbidigoForbidEntry {
    Pattern(String),
    Struct(ForbidigoPatternSettings),
}

/// `linters.settings.forbidigo.forbid[]` struct form.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ForbidigoPatternSettings {
    #[serde(default, alias = "p")]
    pub pattern: String,
    #[serde(default, alias = "pkg")]
    pub pkg: String,
    #[serde(default)]
    pub msg: String,
}

/// `linters.settings.forbidigo` / `linters-settings.forbidigo`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct ForbidigoSettings {
    #[serde(default)]
    pub forbid: Vec<ForbidigoForbidEntry>,
    /// When absent, golangci default is `true`.
    #[serde(default, rename = "exclude-godoc-examples")]
    pub exclude_godoc_examples: Option<bool>,
    #[serde(default, rename = "analyze-types")]
    pub analyze_types: bool,
}

/// `linters.settings.importas` / `linters-settings.importas`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ImportasSettings {
    #[serde(default)]
    pub alias: Vec<ImportasAliasSetting>,
    #[serde(default, rename = "no-unaliased")]
    pub no_unaliased: bool,
    #[serde(default, rename = "no-extra-aliases")]
    pub no_extra_aliases: bool,
}

/// One `importas.alias[]` entry (`pkg` + `alias`).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ImportasAliasSetting {
    #[serde(default)]
    pub pkg: String,
    #[serde(default)]
    pub alias: String,
}

/// `linters.settings.gosmopolitan` / `linters-settings.gosmopolitan`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GosmopolitanSettings {
    #[serde(default, rename = "allow-time-local")]
    pub allow_time_local: bool,
    #[serde(default, rename = "escape-hatches", deserialize_with = "string_or_seq")]
    pub escape_hatches: Vec<String>,
    #[serde(default, rename = "watch-for-scripts", deserialize_with = "string_or_seq")]
    pub watch_for_scripts: Vec<String>,
}

impl GosmopolitanSettings {
    pub fn to_guff_gosmopolitan(&self) -> guff_style::GosmopolitanOptions {
        let defaults = guff_style::GosmopolitanOptions::default();
        guff_style::GosmopolitanOptions {
            allow_time_local: self.allow_time_local,
            escape_hatches: self.escape_hatches.clone(),
            watch_for_scripts: if self.watch_for_scripts.is_empty() {
                defaults.watch_for_scripts
            } else {
                self.watch_for_scripts.clone()
            },
        }
    }
}

/// `linters.settings.goheader` / `linters-settings.goheader`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GoheaderSettings {
    #[serde(default)]
    pub template: String,
    #[serde(default, rename = "template-path")]
    pub template_path: String,
    #[serde(default)]
    pub values: GoheaderValues,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GoheaderValues {
    #[serde(default, rename = "const")]
    pub const_values: std::collections::HashMap<String, String>,
    #[serde(default, rename = "regexp")]
    pub regexp_values: std::collections::HashMap<String, String>,
}

impl GoheaderSettings {
    pub fn to_guff_goheader(&self) -> guff_style::GoheaderOptions {
        guff_style::GoheaderOptions {
            template: self.template.clone(),
            template_path: self.template_path.clone(),
            const_values: self.values.const_values.clone(),
            regexp_values: self.values.regexp_values.clone(),
        }
    }
}

/// `linters.settings.asasalint` / `linters-settings.asasalint`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AsasalintSettings {
    #[serde(default, deserialize_with = "string_or_seq")]
    pub exclude: Vec<String>,
    /// golangci default `true`. When false, only `exclude` patterns apply.
    #[serde(default = "default_true", rename = "use-builtin-exclusions")]
    pub use_builtin_exclusions: bool,
}

impl Default for AsasalintSettings {
    fn default() -> Self {
        Self {
            exclude: Vec::new(),
            use_builtin_exclusions: true,
        }
    }
}

/// `linters.settings.reassign` / `linters-settings.reassign`.
///
/// Empty `patterns` → upstream default `^(Err.*|EOF)$`.
/// Non-empty → joined as `^(p1|p2|…)$` (golangci-lint compat).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ReassignSettings {
    #[serde(default, deserialize_with = "string_or_seq")]
    pub patterns: Vec<String>,
}

/// `linters.settings.recvcheck` / `linters-settings.recvcheck`.
///
/// `disable-builtin` false (default) keeps Unmarshal*/GobDecode excludes.
/// `exclusions` format: `Struct.Method` or `*.Method`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct RecvcheckSettings {
    #[serde(default, rename = "disable-builtin")]
    pub disable_builtin: bool,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub exclusions: Vec<String>,
}

/// `linters.settings.interfacebloat` / `linters-settings.interfacebloat`.
///
/// `max` is the maximum number of methods allowed inside an interface
/// (upstream default 10).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct InterfacebloatSettings {
    #[serde(default = "default_interfacebloat_max")]
    pub max: usize,
}

fn default_interfacebloat_max() -> usize {
    10
}

impl Default for InterfacebloatSettings {
    fn default() -> Self {
        Self {
            max: default_interfacebloat_max(),
        }
    }
}

impl InterfacebloatSettings {
    pub fn to_guff_interfacebloat(&self) -> guff_style::InterfacebloatOptions {
        guff_style::InterfacebloatOptions { max: self.max }
    }
}

/// `linters.settings.embeddedstructfieldcheck` /
/// `linters-settings.embeddedstructfieldcheck`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct EmbeddedstructfieldcheckSettings {
    /// Require a blank line between embedded and regular fields (default true).
    #[serde(default = "default_true", rename = "empty-line")]
    pub empty_line: bool,
    /// Forbid embedding `sync.Mutex` / `sync.RWMutex` (default false).
    #[serde(default, rename = "forbid-mutex")]
    pub forbid_mutex: bool,
}

impl Default for EmbeddedstructfieldcheckSettings {
    fn default() -> Self {
        Self {
            empty_line: true,
            forbid_mutex: false,
        }
    }
}

impl EmbeddedstructfieldcheckSettings {
    pub fn to_guff_embeddedstructfieldcheck(
        &self,
    ) -> guff_style::EmbeddedstructfieldcheckOptions {
        guff_style::EmbeddedstructfieldcheckOptions {
            empty_line: self.empty_line,
            forbid_mutex: self.forbid_mutex,
        }
    }
}

/// `linters.settings.gochecksumtype` / `linters-settings.gochecksumtype`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GochecksumtypeSettings {
    /// Presence of a non-panicking `default` satisfies exhaustiveness.
    /// Upstream / golangci default: true.
    #[serde(default = "default_true", rename = "default-signifies-exhaustive")]
    pub default_signifies_exhaustive: bool,
    /// Include shared interfaces in the exhaustiveness check.
    /// Upstream / golangci default: false.
    #[serde(default, rename = "include-shared-interfaces")]
    pub include_shared_interfaces: bool,
}

impl Default for GochecksumtypeSettings {
    fn default() -> Self {
        Self {
            default_signifies_exhaustive: true,
            include_shared_interfaces: false,
        }
    }
}

impl GochecksumtypeSettings {
    pub fn to_guff_gochecksumtype(&self) -> guff_style::GochecksumtypeOptions {
        guff_style::GochecksumtypeOptions {
            default_signifies_exhaustive: self.default_signifies_exhaustive,
            include_shared_interfaces: self.include_shared_interfaces,
        }
    }
}

/// `linters.settings.inamedparam` / `linters-settings.inamedparam`.
///
/// `skip-single-param` skips methods with exactly one parameter field
/// (upstream default false).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct InamedparamSettings {
    #[serde(default, rename = "skip-single-param")]
    pub skip_single_param: bool,
}

impl InamedparamSettings {
    pub fn to_guff_inamedparam(&self) -> guff_style::InamedparamOptions {
        guff_style::InamedparamOptions {
            skip_single_param: self.skip_single_param,
        }
    }
}

/// `linters.settings.nonamedreturns` / `linters-settings.nonamedreturns`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct NonamedreturnsSettings {
    /// Report named `error` returns even when used in defer (upstream default false).
    #[serde(default, rename = "report-error-in-defer")]
    pub report_error_in_defer: bool,
    /// Allow unused named returns; report only if referenced or used by naked return.
    #[serde(default, rename = "allow-unused-named-returns")]
    pub allow_unused_named_returns: bool,
}

impl NonamedreturnsSettings {
    pub fn to_guff_nonamedreturns(&self) -> guff_style::NonamedreturnsOptions {
        guff_style::NonamedreturnsOptions {
            report_error_in_defer: self.report_error_in_defer,
            allow_unused_named_returns: self.allow_unused_named_returns,
        }
    }
}

/// `linters.settings.funcorder` / `linters-settings.funcorder`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct FuncorderSettings {
    /// Check constructors are placed after the struct declaration (default true).
    #[serde(default = "default_true")]
    pub constructor: bool,
    /// Check exported methods precede unexported methods (default true).
    #[serde(default = "default_true", rename = "struct-method")]
    pub struct_method: bool,
    /// Check constructors / methods are sorted alphabetically (default false).
    #[serde(default)]
    pub alphabetical: bool,
    /// Check exported functions precede unexported functions (default false).
    #[serde(default)]
    pub function: bool,
}

impl Default for FuncorderSettings {
    fn default() -> Self {
        Self {
            constructor: true,
            struct_method: true,
            alphabetical: false,
            function: false,
        }
    }
}

impl FuncorderSettings {
    pub fn to_guff_funcorder(&self) -> guff_style::FuncorderOptions {
        guff_style::FuncorderOptions {
            constructor: self.constructor,
            struct_method: self.struct_method,
            alphabetical: self.alphabetical,
            function: self.function,
        }
    }
}

/// `linters.settings.varnamelen` / `linters-settings.varnamelen`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct VarnamelenSettings {
    #[serde(default = "default_varnamelen_max_distance", rename = "max-distance")]
    pub max_distance: usize,
    #[serde(
        default = "default_varnamelen_min_name_length",
        rename = "min-name-length"
    )]
    pub min_name_length: usize,
    #[serde(default, rename = "check-receiver")]
    pub check_receiver: bool,
    #[serde(default, rename = "check-return")]
    pub check_return: bool,
    #[serde(default, rename = "check-type-param")]
    pub check_type_param: bool,
    #[serde(default, rename = "ignore-type-assert-ok")]
    pub ignore_type_assert_ok: bool,
    #[serde(default, rename = "ignore-map-index-ok")]
    pub ignore_map_index_ok: bool,
    #[serde(default, rename = "ignore-chan-recv-ok")]
    pub ignore_chan_recv_ok: bool,
    #[serde(default, rename = "ignore-names", deserialize_with = "string_or_seq")]
    pub ignore_names: Vec<String>,
    #[serde(default, rename = "ignore-decls", deserialize_with = "string_or_seq")]
    pub ignore_decls: Vec<String>,
}

fn default_varnamelen_max_distance() -> usize {
    5
}

fn default_varnamelen_min_name_length() -> usize {
    3
}

impl Default for VarnamelenSettings {
    fn default() -> Self {
        Self {
            max_distance: default_varnamelen_max_distance(),
            min_name_length: default_varnamelen_min_name_length(),
            check_receiver: false,
            check_return: false,
            check_type_param: false,
            ignore_type_assert_ok: false,
            ignore_map_index_ok: false,
            ignore_chan_recv_ok: false,
            ignore_names: Vec::new(),
            ignore_decls: Vec::new(),
        }
    }
}

impl VarnamelenSettings {
    pub fn to_guff_varnamelen(&self) -> guff_style::VarnamelenOptions {
        guff_style::VarnamelenOptions {
            max_distance: if self.max_distance > 0 {
                self.max_distance
            } else {
                default_varnamelen_max_distance()
            },
            min_name_length: if self.min_name_length > 0 {
                self.min_name_length
            } else {
                default_varnamelen_min_name_length()
            },
            check_receiver: self.check_receiver,
            check_return: self.check_return,
            check_type_param: self.check_type_param,
            ignore_type_assert_ok: self.ignore_type_assert_ok,
            ignore_map_index_ok: self.ignore_map_index_ok,
            ignore_chan_recv_ok: self.ignore_chan_recv_ok,
            ignore_names: self.ignore_names.clone(),
            ignore_decls: self.ignore_decls.clone(),
        }
    }
}

/// `linters.settings.unparam` / `linters-settings.unparam`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct UnparamSettings {
    #[serde(default, rename = "check-exported")]
    pub check_exported: bool,
}

impl UnparamSettings {
    pub fn to_guff_unparam(&self) -> guff_style::UnparamOptions {
        guff_style::UnparamOptions {
            check_exported: self.check_exported,
        }
    }
}

/// `linters.settings.unqueryvet` / `linters-settings.unqueryvet`.
///
/// Core SELECT * keys only. SQL builders / N+1 / injection / tx-leak / custom
/// DSL are DEFERRED.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct UnqueryvetSettings {
    #[serde(default = "default_true", rename = "check-aliased-wildcard")]
    pub check_aliased_wildcard: bool,
    #[serde(default = "default_true", rename = "check-subqueries")]
    pub check_subqueries: bool,
    /// Empty → upstream defaults (`COUNT(*)`, system catalogs, …).
    #[serde(default, rename = "allowed-patterns", deserialize_with = "string_or_seq")]
    pub allowed_patterns: Vec<String>,
}

impl Default for UnqueryvetSettings {
    fn default() -> Self {
        Self {
            check_aliased_wildcard: true,
            check_subqueries: true,
            allowed_patterns: Vec::new(),
        }
    }
}

impl UnqueryvetSettings {
    pub fn to_guff_unqueryvet(&self) -> guff_style::UnqueryvetOptions {
        guff_style::UnqueryvetOptions {
            check_aliased_wildcard: self.check_aliased_wildcard,
            check_subqueries: self.check_subqueries,
            allowed_patterns: self.allowed_patterns.clone(),
        }
    }
}

/// `linters.settings.promlinter` / `linters-settings.promlinter`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct PromlinterSettings {
    /// Report parse failures (DEFERRED; accepted for config compat).
    #[serde(default)]
    pub strict: bool,
    /// Disable named promlint checks (`Help`, `Counter`, `CamelCase`, …).
    #[serde(default, rename = "disabled-linters", deserialize_with = "string_or_seq")]
    pub disabled_linters: Vec<String>,
}

impl PromlinterSettings {
    pub fn to_guff_promlinter(&self) -> guff_style::PromlinterOptions {
        guff_style::PromlinterOptions {
            strict: self.strict,
            disabled_linters: self.disabled_linters.clone(),
        }
    }
}

/// `linters.settings.ginkgolinter` / `linters-settings.ginkgolinter`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GinkgolinterSettings {
    #[serde(default, rename = "suppress-len-assertion")]
    pub suppress_len_assertion: bool,
    #[serde(default, rename = "suppress-nil-assertion")]
    pub suppress_nil_assertion: bool,
    #[serde(default, rename = "suppress-err-assertion")]
    pub suppress_err_assertion: bool,
    #[serde(default, rename = "suppress-compare-assertion")]
    pub suppress_compare_assertion: bool,
    #[serde(default, rename = "suppress-async-assertion")]
    pub suppress_async_assertion: bool,
    #[serde(default, rename = "suppress-type-compare-assertion")]
    pub suppress_type_compare_assertion: bool,
    #[serde(default, rename = "forbid-focus-container")]
    pub forbid_focus_container: bool,
    #[serde(default, rename = "allow-havelen-zero")]
    pub allow_havelen_zero: bool,
    #[serde(default, rename = "force-expect-to")]
    pub force_expect_to: bool,
    #[serde(default, rename = "validate-async-intervals")]
    pub validate_async_intervals: bool,
    #[serde(default, rename = "forbid-spec-pollution")]
    pub forbid_spec_pollution: bool,
    #[serde(default, rename = "force-succeed")]
    pub force_succeed: bool,
    #[serde(default, rename = "force-assertion-description")]
    pub force_assertion_description: bool,
    #[serde(default, rename = "force-tonot")]
    pub force_tonot: bool,
}

impl GinkgolinterSettings {
    pub fn to_guff_ginkgolinter(&self) -> guff_style::GinkgolinterOptions {
        guff_style::GinkgolinterOptions {
            suppress_len_assertion: self.suppress_len_assertion,
            suppress_nil_assertion: self.suppress_nil_assertion,
            suppress_err_assertion: self.suppress_err_assertion,
            suppress_compare_assertion: self.suppress_compare_assertion,
            suppress_async_assertion: self.suppress_async_assertion,
            suppress_type_compare_assertion: self.suppress_type_compare_assertion,
            forbid_focus_container: self.forbid_focus_container,
            allow_havelen_zero: self.allow_havelen_zero,
            force_expect_to: self.force_expect_to,
            validate_async_intervals: self.validate_async_intervals,
            forbid_spec_pollution: self.forbid_spec_pollution,
            force_succeed: self.force_succeed,
            force_assertion_description: self.force_assertion_description,
            force_tonot: self.force_tonot,
        }
    }
}

/// `linters.settings.wsl_v5` / `linters-settings.wsl_v5`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct WslV5Settings {
    #[serde(default, rename = "allow-first-in-block")]
    pub allow_first_in_block: Option<bool>,
    #[serde(default, rename = "allow-whole-block")]
    pub allow_whole_block: Option<bool>,
    #[serde(default, rename = "branch-max-lines")]
    pub branch_max_lines: Option<usize>,
    #[serde(default, rename = "case-max-lines")]
    pub case_max_lines: Option<usize>,
    #[serde(default, rename = "cuddle-max-statements")]
    pub cuddle_max_statements: Option<usize>,
    /// Preset: `all` / `none` / `default` / empty.
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub enable: Vec<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub disable: Vec<String>,
}

impl WslV5Settings {
    pub fn to_guff_wsl_v5(&self) -> guff_style::WslV5Options {
        let defaults = guff_style::WslV5Options::default();
        let preset = self.default.as_deref().unwrap_or("");
        guff_style::WslV5Options {
            allow_first_in_block: self
                .allow_first_in_block
                .unwrap_or(defaults.allow_first_in_block),
            allow_whole_block: self
                .allow_whole_block
                .unwrap_or(defaults.allow_whole_block),
            branch_max_lines: self.branch_max_lines.unwrap_or(defaults.branch_max_lines),
            case_max_lines: self.case_max_lines.unwrap_or(defaults.case_max_lines),
            cuddle_max_statements: self
                .cuddle_max_statements
                .unwrap_or(defaults.cuddle_max_statements),
            checks: guff_style::WslV5Options::resolve_checks(
                preset,
                &self.enable,
                &self.disable,
            ),
        }
    }
}

/// `linters.settings.paralleltest` / `linters-settings.paralleltest`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ParalleltestSettings {
    #[serde(default, rename = "ignore-missing")]
    pub ignore_missing: bool,
    #[serde(default, rename = "ignore-missing-subtests")]
    pub ignore_missing_subtests: bool,
    #[serde(default, rename = "check-cleanup")]
    pub check_cleanup: bool,
}

impl ParalleltestSettings {
    pub fn to_guff_paralleltest(&self) -> guff_style::ParalleltestOptions {
        guff_style::ParalleltestOptions {
            ignore_missing: self.ignore_missing,
            ignore_missing_subtests: self.ignore_missing_subtests,
            check_cleanup: self.check_cleanup,
        }
    }
}

/// `linters.settings.testpackage` / `linters-settings.testpackage`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TestpackageSettings {
    /// Regexp matched against the test file path; matches are skipped.
    #[serde(default = "default_testpackage_skip_regexp", rename = "skip-regexp")]
    pub skip_regexp: String,
    /// Package names that may appear in `*_test.go` without a `_test` suffix.
    #[serde(default = "default_testpackage_allow_packages", rename = "allow-packages", deserialize_with = "string_or_seq")]
    pub allow_packages: Vec<String>,
}

fn default_testpackage_skip_regexp() -> String {
    r"(export|internal)_test\.go".into()
}

fn default_testpackage_allow_packages() -> Vec<String> {
    vec!["main".into()]
}

impl Default for TestpackageSettings {
    fn default() -> Self {
        Self {
            skip_regexp: default_testpackage_skip_regexp(),
            allow_packages: default_testpackage_allow_packages(),
        }
    }
}

impl TestpackageSettings {
    pub fn to_guff_testpackage(&self) -> guff_style::TestpackageOptions {
        guff_style::TestpackageOptions {
            skip_regexp: self.skip_regexp.clone(),
            allow_packages: self.allow_packages.clone(),
        }
    }
}

/// `linters.settings.tagliatelle` / `linters-settings.tagliatelle`.
///
/// User `case.rules` are merged onto golangci defaults
/// (`json`/`yaml` → `camel`, `header` → `header`).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TagliatelleSettings {
    #[serde(default)]
    pub case: TagliatelleCaseSettings,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TagliatelleCaseSettings {
    #[serde(default)]
    pub rules: std::collections::HashMap<String, String>,
    #[serde(default, rename = "extended-rules")]
    pub extended_rules: std::collections::HashMap<String, TagliatelleExtendedRuleSettings>,
    #[serde(default, rename = "use-field-name")]
    pub use_field_name: bool,
    #[serde(default, rename = "ignored-fields", deserialize_with = "string_or_seq")]
    pub ignored_fields: Vec<String>,
    // DEFERRED: overrides (package radix tree).
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TagliatelleExtendedRuleSettings {
    #[serde(default, rename = "case")]
    pub case_name: String,
    // DEFERRED: extra-initialisms / initialism-overrides.
}

impl TagliatelleSettings {
    pub fn to_guff_tagliatelle(&self) -> guff_style::TagliatelleOptions {
        let mut extended = std::collections::HashMap::new();
        for (k, v) in &self.case.extended_rules {
            if !v.case_name.is_empty() {
                extended.insert(k.clone(), v.case_name.clone());
            }
        }
        guff_style::TagliatelleOptions {
            rules: self.case.rules.clone(),
            extended_rules: extended,
            use_field_name: self.case.use_field_name,
            ignored_fields: self.case.ignored_fields.clone(),
            ignore: false,
        }
    }
}

/// `linters.settings.decorder` / `linters-settings.decorder`.
///
/// Golangci-lint defaults disable the three check families.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DecorderSettings {
    #[serde(default = "default_dec_order", rename = "dec-order", deserialize_with = "string_or_seq")]
    pub dec_order: Vec<String>,
    #[serde(default, rename = "ignore-underscore-vars")]
    pub ignore_underscore_vars: bool,
    #[serde(default = "default_true", rename = "disable-dec-num-check")]
    pub disable_dec_num_check: bool,
    #[serde(default, rename = "disable-type-dec-num-check")]
    pub disable_type_dec_num_check: bool,
    #[serde(default, rename = "disable-const-dec-num-check")]
    pub disable_const_dec_num_check: bool,
    #[serde(default, rename = "disable-var-dec-num-check")]
    pub disable_var_dec_num_check: bool,
    #[serde(default = "default_true", rename = "disable-dec-order-check")]
    pub disable_dec_order_check: bool,
    #[serde(default = "default_true", rename = "disable-init-func-first-check")]
    pub disable_init_func_first_check: bool,
}

fn default_dec_order() -> Vec<String> {
    vec![
        "type".into(),
        "const".into(),
        "var".into(),
        "func".into(),
    ]
}

impl Default for DecorderSettings {
    fn default() -> Self {
        Self {
            dec_order: default_dec_order(),
            ignore_underscore_vars: false,
            disable_dec_num_check: true,
            disable_type_dec_num_check: false,
            disable_const_dec_num_check: false,
            disable_var_dec_num_check: false,
            disable_dec_order_check: true,
            disable_init_func_first_check: true,
        }
    }
}

impl DecorderSettings {
    pub fn to_guff_decorder(&self) -> guff_style::DecorderOptions {
        guff_style::DecorderOptions {
            dec_order: if self.dec_order.is_empty() {
                default_dec_order()
            } else {
                self.dec_order.clone()
            },
            ignore_underscore_vars: self.ignore_underscore_vars,
            disable_dec_num_check: self.disable_dec_num_check,
            disable_type_dec_num_check: self.disable_type_dec_num_check,
            disable_const_dec_num_check: self.disable_const_dec_num_check,
            disable_var_dec_num_check: self.disable_var_dec_num_check,
            disable_dec_order_check: self.disable_dec_order_check,
            disable_init_func_first_check: self.disable_init_func_first_check,
        }
    }
}

/// `linters.settings.iotamixing` / `linters-settings.iotamixing`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct IotamixingSettings {
    /// Report each valued const instead of the whole block (default false).
    #[serde(default, rename = "report-individual")]
    pub report_individual: bool,
}

impl IotamixingSettings {
    pub fn to_guff_iotamixing(&self) -> guff_style::IotamixingOptions {
        guff_style::IotamixingOptions {
            report_individual: self.report_individual,
        }
    }
}

/// `linters.settings.grouper` / `linters-settings.grouper`.
///
/// All flags default to false (golangci / upstream).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GrouperSettings {
    #[serde(default, rename = "const-require-single-const")]
    pub const_require_single_const: bool,
    #[serde(default, rename = "const-require-grouping")]
    pub const_require_grouping: bool,
    #[serde(default, rename = "import-require-single-import")]
    pub import_require_single_import: bool,
    #[serde(default, rename = "import-require-grouping")]
    pub import_require_grouping: bool,
    #[serde(default, rename = "type-require-single-type")]
    pub type_require_single_type: bool,
    #[serde(default, rename = "type-require-grouping")]
    pub type_require_grouping: bool,
    #[serde(default, rename = "var-require-single-var")]
    pub var_require_single_var: bool,
    #[serde(default, rename = "var-require-grouping")]
    pub var_require_grouping: bool,
}

impl GrouperSettings {
    pub fn to_guff_grouper(&self) -> guff_style::GrouperOptions {
        guff_style::GrouperOptions {
            const_require_single_const: self.const_require_single_const,
            const_require_grouping: self.const_require_grouping,
            import_require_single_import: self.import_require_single_import,
            import_require_grouping: self.import_require_grouping,
            type_require_single_type: self.type_require_single_type,
            type_require_grouping: self.type_require_grouping,
            var_require_single_var: self.var_require_single_var,
            var_require_grouping: self.var_require_grouping,
        }
    }
}

/// `linters.settings.ireturn` / `linters-settings.ireturn`.
///
/// Default (both empty): allow `anon` / `error` / `empty` / `stdlib`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct IreturnSettings {
    #[serde(default, deserialize_with = "string_or_seq")]
    pub allow: Vec<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub reject: Vec<String>,
}

impl IreturnSettings {
    pub fn to_guff_ireturn(&self) -> guff_style::IreturnOptions {
        guff_style::IreturnOptions {
            allow: self.allow.clone(),
            reject: self.reject.clone(),
        }
    }
}

/// `linters.settings.gosec` / `linters-settings.gosec`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct GosecSettings {
    #[serde(default, deserialize_with = "string_or_seq")]
    pub includes: Vec<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub excludes: Vec<String>,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub confidence: String,
    /// Per-rule `config` map. Only the `G101` sub-map is interpreted; the rest
    /// (`G301`, `global`, …) is DEFERRED.
    #[serde(default)]
    pub config: Option<serde_yaml::Value>,
    // DEFERRED: concurrency.
}

/// Read one key out of a `config.<rule>` sub-map.
///
/// Upstream gosec takes the numeric G101 knobs as **strings** and keeps the
/// default when `strconv` fails; YAML that writes them unquoted still parses
/// here, which is strictly more forgiving and never changes a valid config.
fn gosec_config_key<'a>(
    config: &'a serde_yaml::Value,
    rule: &str,
    key: &str,
) -> Option<&'a serde_yaml::Value> {
    config
        .get(serde_yaml::Value::String(rule.to_string()))?
        .get(serde_yaml::Value::String(key.to_string()))
}

fn gosec_config_f64(config: &serde_yaml::Value, rule: &str, key: &str) -> Option<f64> {
    let v = gosec_config_key(config, rule, key)?;
    v.as_f64().or_else(|| v.as_str()?.trim().parse().ok())
}

fn gosec_config_usize(config: &serde_yaml::Value, rule: &str, key: &str) -> Option<usize> {
    let v = gosec_config_key(config, rule, key)?;
    v.as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .or_else(|| v.as_str()?.trim().parse().ok())
}

impl GosecSettings {
    pub fn to_guff_gosec(&self) -> guff_style::GosecOptions {
        let mut g101 = guff_style::G101Options::default();
        if let Some(config) = &self.config {
            if let Some(p) = gosec_config_key(config, "G101", "pattern").and_then(|v| v.as_str()) {
                g101.pattern = p.to_string();
            }
            if let Some(b) =
                gosec_config_key(config, "G101", "ignore_entropy").and_then(|v| v.as_bool())
            {
                g101.ignore_entropy = b;
            }
            if let Some(n) = gosec_config_f64(config, "G101", "entropy_threshold") {
                g101.entropy_threshold = n;
            }
            if let Some(n) = gosec_config_f64(config, "G101", "per_char_threshold") {
                g101.per_char_threshold = n;
            }
            if let Some(n) = gosec_config_usize(config, "G101", "truncate") {
                g101.truncate = n;
            }
            if let Some(n) = gosec_config_usize(config, "G101", "min_entropy_length") {
                g101.min_entropy_length = n;
            }
        }
        guff_style::GosecOptions {
            includes: self.includes.clone(),
            excludes: self.excludes.clone(),
            severity: self.severity.clone(),
            confidence: self.confidence.clone(),
            g101,
        }
    }
}

/// `linters.settings.nolintlint` / `linters-settings.nolintlint`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct NolintlintSettings {
    /// When true, do not report unused `//nolint` directives (golangci default: false).
    #[serde(default, rename = "allow-unused")]
    pub allow_unused: bool,
    // DEFERRED: allow-leading-space / require-explanation / require-specific.
}

/// One of `thelper.{test,fuzz,benchmark,tb}` option groups.
///
/// `None` fields keep upstream defaults (all checks enabled).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ThelperKindSettings {
    #[serde(default)]
    pub first: Option<bool>,
    #[serde(default)]
    pub name: Option<bool>,
    #[serde(default)]
    pub begin: Option<bool>,
}

/// `linters.settings.thelper` / `linters-settings.thelper`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ThelperSettings {
    #[serde(default)]
    pub test: ThelperKindSettings,
    #[serde(default)]
    pub fuzz: ThelperKindSettings,
    #[serde(default)]
    pub benchmark: ThelperKindSettings,
    #[serde(default)]
    pub tb: ThelperKindSettings,
}

/// Nested `linters.settings.iface.settings.unused`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct IfaceUnusedSettings {
    /// Exact package paths to skip (golangci `settings.unused.exclude`).
    #[serde(default, deserialize_with = "string_or_seq")]
    pub exclude: Vec<String>,
}

/// Nested `linters.settings.iface.settings`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct IfaceNestedSettings {
    #[serde(default)]
    pub unused: IfaceUnusedSettings,
}

/// `linters.settings.iface` / `linters-settings.iface`.
///
/// Empty `enable` → golangci default (`identical` only).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct IfaceSettings {
    #[serde(default, deserialize_with = "string_or_seq")]
    pub enable: Vec<String>,
    #[serde(default)]
    pub settings: IfaceNestedSettings,
}

/// `linters.settings.bidichk` / `linters-settings.bidichk`.
///
/// Each bool enables checking for that rune. When all are false (golangci
/// default), all nine dangerous runes are checked — matching upstream
/// `disallowed-runes` empty-string behavior.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct BidichkSettings {
    #[serde(default, rename = "left-to-right-embedding")]
    pub left_to_right_embedding: bool,
    #[serde(default, rename = "right-to-left-embedding")]
    pub right_to_left_embedding: bool,
    #[serde(default, rename = "pop-directional-formatting")]
    pub pop_directional_formatting: bool,
    #[serde(default, rename = "left-to-right-override")]
    pub left_to_right_override: bool,
    #[serde(default, rename = "right-to-left-override")]
    pub right_to_left_override: bool,
    #[serde(default, rename = "left-to-right-isolate")]
    pub left_to_right_isolate: bool,
    #[serde(default, rename = "right-to-left-isolate")]
    pub right_to_left_isolate: bool,
    #[serde(default, rename = "first-strong-isolate")]
    pub first_strong_isolate: bool,
    #[serde(default, rename = "pop-directional-isolate")]
    pub pop_directional_isolate: bool,
}

/// `linters.settings.depguard` / `linters-settings.depguard`.
///
/// Empty `rules` → analyzer default (`Main` / `$gostd` only).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DepguardSettings {
    #[serde(default)]
    pub rules: std::collections::BTreeMap<String, DepguardRuleSetting>,
}

/// One depguard rule entry (YAML value under `rules.<name>`).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DepguardRuleSetting {
    #[serde(default, rename = "list-mode")]
    pub list_mode: Option<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub files: Vec<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<DepguardDenySetting>,
}

/// One `deny` entry (`pkg` + optional `desc`).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DepguardDenySetting {
    #[serde(default)]
    pub pkg: String,
    #[serde(default)]
    pub desc: String,
}

/// `linters.settings.gomoddirectives` / `linters-settings.gomoddirectives`.
///
/// `ignore-forbidden` / `toolchain-pattern` / `go-version-pattern` /
/// `check-module-path` remain DEFERRED.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GomoddirectivesSettings {
    #[serde(default, rename = "replace-local")]
    pub replace_local: bool,
    #[serde(default, rename = "replace-allow-list", deserialize_with = "string_or_seq")]
    pub replace_allow_list: Vec<String>,
    #[serde(default, rename = "retract-allow-no-explanation")]
    pub retract_allow_no_explanation: bool,
    #[serde(default, rename = "exclude-forbidden")]
    pub exclude_forbidden: bool,
    #[serde(default, rename = "toolchain-forbidden")]
    pub toolchain_forbidden: bool,
    #[serde(default, rename = "tool-forbidden")]
    pub tool_forbidden: bool,
    #[serde(default, rename = "go-debug-forbidden")]
    pub go_debug_forbidden: bool,
}

/// Combined `gomodguard` + `gomodguard_v2` settings (same Pass bag key).
///
/// Allowed modules/domains, version constraints, and `match-type` remain DEFERRED.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GomodguardSettings {
    pub blocked_modules: Vec<(String, String)>,
    pub local_replace_directives: bool,
}

/// Deserialize one `linters.settings.<name>` block, reporting failures.
///
/// A malformed block used to fall back to defaults silently, which is the
/// worst outcome available: guff lints with settings the user did not ask for
/// and reports findings they cannot explain from their config. golangci-lint
/// fails the run outright; guff keeps going so one bad block cannot take down
/// the whole lint, but the fallback is never silent.
fn parse_settings<T: serde::de::DeserializeOwned>(
    name: &str,
    value: &serde_yaml::Value,
) -> Option<T> {
    match serde_yaml::from_value::<T>(value.clone()) {
        Ok(parsed) => Some(parsed),
        Err(err) => {
            eprintln!(
                "guff: ignoring linters.settings.{name} ({err}); \
                 falling back to that linter's defaults"
            );
            None
        }
    }
}

/// Read one `gocritic.settings.<check>.<param>` value.
///
/// go-critic parses params through `linter.CheckerParams`, which coerces
/// strings, so both `maxResults: 10` and `maxResults: "10"` are accepted.
fn gocritic_param<'a>(
    settings: &'a serde_yaml::Value,
    check: &str,
    param: &str,
) -> Option<&'a serde_yaml::Value> {
    settings
        .get(serde_yaml::Value::String(check.to_string()))?
        .get(serde_yaml::Value::String(param.to_string()))
}

fn gocritic_param_usize(settings: &serde_yaml::Value, check: &str, param: &str) -> Option<usize> {
    let v = gocritic_param(settings, check, param)?;
    v.as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .or_else(|| v.as_str()?.trim().parse().ok())
}

fn gocritic_param_bool(settings: &serde_yaml::Value, check: &str, param: &str) -> Option<bool> {
    let v = gocritic_param(settings, check, param)?;
    v.as_bool().or_else(|| v.as_str()?.trim().parse().ok())
}

/// Accept either a bare string or a list of strings.
///
/// golangci-lint decodes its config with mapstructure's `WeaklyTypedInput`, so
/// `ignore-string-values: foo.+` and `ignore-string-values: [foo.+]` are the
/// same config there. Without this, the scalar form fails to deserialize and
/// (via [`parse_settings`]) discards *every* setting for that linter — the
/// visible symptom is unrelated options like `ignore-calls` reverting to their
/// defaults.
pub(crate) fn string_or_seq<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }

    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    })
}

impl LinterSettings {
    /// Parse from v2 `linters.settings` or v1 `linters-settings` YAML mapping.
    pub fn from_yaml(value: &serde_yaml::Value) -> Self {
        let Some(map) = value.as_mapping() else {
            return Self::default();
        };
        let mut out = Self::default();
        if let Some(v) = map.get(serde_yaml::Value::String("errcheck".into())) {
            if let Some(s) = parse_settings::<ErrcheckSettings>("errcheck", v) {
                out.errcheck = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("govet".into())) {
            if let Some(s) = parse_settings::<GovetSettings>("govet", v) {
                out.govet = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("staticcheck".into())) {
            if let Some(s) = parse_settings::<StaticcheckSettings>("staticcheck", v) {
                out.staticcheck = s;
            }
        }
        // golangci v1 / many OSS configs put ST* keys under `stylecheck`.
        if let Some(v) = map.get(serde_yaml::Value::String("stylecheck".into())) {
            if let Some(s) = parse_settings::<StaticcheckSettings>("stylecheck", v) {
                out.staticcheck.merge_stylecheck(s);
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("revive".into())) {
            if let Some(s) = parse_settings::<ReviveSettings>("revive", v) {
                out.revive = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("dupl".into())) {
            if let Some(s) = parse_settings::<DuplSettings>("dupl", v) {
                out.dupl = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("misspell".into())) {
            if let Some(s) = parse_settings::<MisspellSettings>("misspell", v) {
                out.misspell = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("gocyclo".into())) {
            if let Some(s) = parse_settings::<GocycloSettings>("gocyclo", v) {
                out.gocyclo = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("maintidx".into())) {
            if let Some(s) = parse_settings::<MaintidxSettings>("maintidx", v) {
                out.maintidx = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("gocognit".into())) {
            if let Some(s) = parse_settings::<GocognitSettings>("gocognit", v) {
                out.gocognit = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("nestif".into())) {
            if let Some(s) = parse_settings::<NestifSettings>("nestif", v) {
                out.nestif = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("dogsled".into())) {
            if let Some(s) = parse_settings::<DogsledSettings>("dogsled", v) {
                out.dogsled = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("funlen".into())) {
            if let Some(s) = parse_settings::<FunlenSettings>("funlen", v) {
                out.funlen = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("cyclop".into())) {
            if let Some(s) = parse_settings::<CyclopSettings>("cyclop", v) {
                out.cyclop = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("lll".into())) {
            if let Some(s) = parse_settings::<LllSettings>("lll", v) {
                out.lll = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("nakedret".into())) {
            if let Some(s) = parse_settings::<NakedretSettings>("nakedret", v) {
                out.nakedret = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("nlreturn".into())) {
            if let Some(s) = parse_settings::<NlreturnSettings>("nlreturn", v) {
                out.nlreturn = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("predeclared".into())) {
            if let Some(s) = parse_settings::<PredeclaredSettings>("predeclared", v) {
                out.predeclared = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("whitespace".into())) {
            if let Some(s) = parse_settings::<WhitespaceSettings>("whitespace", v) {
                out.whitespace = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("mnd".into())) {
            if let Some(s) = parse_settings::<MndSettings>("mnd", v) {
                out.mnd = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("prealloc".into())) {
            if let Some(s) = parse_settings::<PreallocSettings>("prealloc", v) {
                out.prealloc = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("tagalign".into())) {
            if let Some(s) = parse_settings::<TagalignSettings>("tagalign", v) {
                out.tagalign = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("wsl".into())) {
            if let Some(s) = parse_settings::<WslSettings>("wsl", v) {
                out.wsl = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("perfsprint".into())) {
            if let Some(s) = parse_settings::<PerfsprintSettings>("perfsprint", v) {
                out.perfsprint = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("goconst".into())) {
            if let Some(s) = parse_settings::<GoconstSettings>("goconst", v) {
                out.goconst = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("copyloopvar".into())) {
            if let Some(s) = parse_settings::<CopyloopvarSettings>("copyloopvar", v) {
                out.copyloopvar = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("usetesting".into())) {
            if let Some(s) = parse_settings::<UsetestingSettings>("usetesting", v) {
                out.usetesting = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("usestdlibvars".into())) {
            if let Some(s) = parse_settings::<UsestdlibvarsSettings>("usestdlibvars", v) {
                out.usestdlibvars = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("unconvert".into())) {
            if let Some(s) = parse_settings::<UnconvertSettings>("unconvert", v) {
                out.unconvert = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("exhaustruct".into())) {
            if let Some(s) = parse_settings::<ExhaustructSettings>("exhaustruct", v) {
                out.exhaustruct = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("exhaustive".into())) {
            if let Some(s) = parse_settings::<ExhaustiveSettings>("exhaustive", v) {
                out.exhaustive = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("musttag".into())) {
            if let Some(s) = parse_settings::<MusttagSettings>("musttag", v) {
                out.musttag = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("loggercheck".into())) {
            if let Some(s) = parse_settings::<LoggercheckSettings>("loggercheck", v) {
                out.loggercheck = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("sloglint".into())) {
            if let Some(s) = parse_settings::<SloglintSettings>("sloglint", v) {
                out.sloglint = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("testifylint".into())) {
            if let Some(s) = parse_settings::<TestifylintSettings>("testifylint", v) {
                out.testifylint = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("errchkjson".into())) {
            if let Some(s) = parse_settings::<ErrchkjsonSettings>("errchkjson", v) {
                out.errchkjson = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("wrapcheck".into())) {
            if let Some(s) = parse_settings::<WrapcheckSettings>("wrapcheck", v) {
                out.wrapcheck = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("rowserrcheck".into())) {
            if let Some(s) = parse_settings::<RowserrcheckSettings>("rowserrcheck", v) {
                out.rowserrcheck = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("bodyclose".into())) {
            if let Some(s) = parse_settings::<BodycloseSettings>("bodyclose", v) {
                out.bodyclose = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("godot".into())) {
            if let Some(s) = parse_settings::<GodotSettings>("godot", v) {
                out.godot = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("godox".into())) {
            if let Some(s) = parse_settings::<GodoxSettings>("godox", v) {
                out.godox = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("dupword".into())) {
            if let Some(s) = parse_settings::<DupwordSettings>("dupword", v) {
                out.dupword = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("godoclint".into())) {
            if let Some(s) = parse_settings::<GodoclintSettings>("godoclint", v) {
                out.godoclint = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("depguard".into())) {
            if let Some(s) = parse_settings::<DepguardSettings>("depguard", v) {
                out.depguard = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("gomoddirectives".into())) {
            if let Some(s) = parse_settings::<GomoddirectivesSettings>("gomoddirectives", v) {
                out.gomoddirectives = s;
            }
        }
        // gomodguard (v1) and gomodguard_v2 both feed GomodguardSettings.
        if let Some(v) = map.get(serde_yaml::Value::String("gomodguard".into())) {
            merge_gomodguard_v1(&mut out.gomodguard, v);
        }
        if let Some(v) = map.get(serde_yaml::Value::String("gomodguard_v2".into())) {
            merge_gomodguard_v2(&mut out.gomodguard, v);
        }
        if let Some(v) = map.get(serde_yaml::Value::String("modernize".into())) {
            if let Some(s) = parse_settings::<ModernizeSettings>("modernize", v) {
                out.modernize = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("gocritic".into())) {
            if let Some(s) = parse_settings::<GocriticSettings>("gocritic", v) {
                out.gocritic = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("forbidigo".into())) {
            if let Some(s) = parse_settings::<ForbidigoSettings>("forbidigo", v) {
                out.forbidigo = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("bidichk".into())) {
            if let Some(s) = parse_settings::<BidichkSettings>("bidichk", v) {
                out.bidichk = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("gosmopolitan".into())) {
            if let Some(s) = parse_settings::<GosmopolitanSettings>("gosmopolitan", v) {
                out.gosmopolitan = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("goheader".into())) {
            if let Some(s) = parse_settings::<GoheaderSettings>("goheader", v) {
                out.goheader = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("asasalint".into())) {
            if let Some(s) = parse_settings::<AsasalintSettings>("asasalint", v) {
                out.asasalint = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("reassign".into())) {
            if let Some(s) = parse_settings::<ReassignSettings>("reassign", v) {
                out.reassign = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("recvcheck".into())) {
            if let Some(s) = parse_settings::<RecvcheckSettings>("recvcheck", v) {
                out.recvcheck = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("thelper".into())) {
            if let Some(s) = parse_settings::<ThelperSettings>("thelper", v) {
                out.thelper = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("iface".into())) {
            if let Some(s) = parse_settings::<IfaceSettings>("iface", v) {
                out.iface = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("importas".into())) {
            if let Some(s) = parse_settings::<ImportasSettings>("importas", v) {
                out.importas = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("interfacebloat".into())) {
            if let Some(s) = parse_settings::<InterfacebloatSettings>("interfacebloat", v) {
                out.interfacebloat = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("embeddedstructfieldcheck".into())) {
            if let Some(s) = parse_settings::<EmbeddedstructfieldcheckSettings>("embeddedstructfieldcheck", v) {
                out.embeddedstructfieldcheck = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("gochecksumtype".into())) {
            if let Some(s) = parse_settings::<GochecksumtypeSettings>("gochecksumtype", v) {
                out.gochecksumtype = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("inamedparam".into())) {
            if let Some(s) = parse_settings::<InamedparamSettings>("inamedparam", v) {
                out.inamedparam = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("nonamedreturns".into())) {
            if let Some(s) = parse_settings::<NonamedreturnsSettings>("nonamedreturns", v) {
                out.nonamedreturns = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("funcorder".into())) {
            if let Some(s) = parse_settings::<FuncorderSettings>("funcorder", v) {
                out.funcorder = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("varnamelen".into())) {
            if let Some(s) = parse_settings::<VarnamelenSettings>("varnamelen", v) {
                out.varnamelen = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("unparam".into())) {
            if let Some(s) = parse_settings::<UnparamSettings>("unparam", v) {
                out.unparam = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("unqueryvet".into())) {
            if let Some(s) = parse_settings::<UnqueryvetSettings>("unqueryvet", v) {
                out.unqueryvet = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("promlinter".into())) {
            if let Some(s) = parse_settings::<PromlinterSettings>("promlinter", v) {
                out.promlinter = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("ginkgolinter".into())) {
            if let Some(s) = parse_settings::<GinkgolinterSettings>("ginkgolinter", v) {
                out.ginkgolinter = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("wsl_v5".into())) {
            if let Some(s) = parse_settings::<WslV5Settings>("wsl_v5", v) {
                out.wsl_v5 = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("paralleltest".into())) {
            if let Some(s) = parse_settings::<ParalleltestSettings>("paralleltest", v) {
                out.paralleltest = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("testpackage".into())) {
            if let Some(s) = parse_settings::<TestpackageSettings>("testpackage", v) {
                out.testpackage = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("tagliatelle".into())) {
            if let Some(s) = parse_settings::<TagliatelleSettings>("tagliatelle", v) {
                out.tagliatelle = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("decorder".into())) {
            if let Some(s) = parse_settings::<DecorderSettings>("decorder", v) {
                out.decorder = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("iotamixing".into())) {
            if let Some(s) = parse_settings::<IotamixingSettings>("iotamixing", v) {
                out.iotamixing = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("grouper".into())) {
            if let Some(s) = parse_settings::<GrouperSettings>("grouper", v) {
                out.grouper = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("ireturn".into())) {
            if let Some(s) = parse_settings::<IreturnSettings>("ireturn", v) {
                out.ireturn = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("gosec".into())) {
            if let Some(s) = parse_settings::<GosecSettings>("gosec", v) {
                out.gosec = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("nolintlint".into())) {
            if let Some(s) = parse_settings::<NolintlintSettings>("nolintlint", v) {
                out.nolintlint = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("custom".into())) {
            if let Some(cmap) = v.as_mapping() {
                for (key, val) in cmap {
                    let Some(name) = key.as_str() else {
                        continue;
                    };
                    match serde_yaml::from_value::<CustomLinterConfig>(val.clone()) {
                        Ok(cfg) => {
                            out.custom.insert(name.to_string(), cfg);
                        }
                        Err(e) => {
                            eprintln!("guff: ignoring linters.settings.custom.{name}: {e}");
                        }
                    }
                }
            }
        }
        // Unknown linter keys are intentionally ignored (forward-compat with
        // golangci configs that mention linters guff does not have yet).
        out
    }

    /// Build a [`SettingsBag`] for Pass-time options.
    pub fn to_bag(&self) -> Arc<SettingsBag> {
        let mut bag = SettingsBag::new();
        bag.insert(
            "errcheck",
            guff_errcheck::Options {
                check_blank: self.errcheck.check_blank,
                check_asserts: self.errcheck.check_type_assertions,
                disable_default_exclusions: self.errcheck.disable_default_exclusions,
                exclude_functions: self.errcheck.exclude_functions.clone(),
            },
        );
        bag.insert("staticcheck", self.staticcheck.to_guff_stylecheck());
        // SA4006/SA4031 need SSA DebugRefs (`BuilderMode::GLOBAL_DEBUG`).
        bag.insert(
            "buildir_global_debug",
            staticcheck_check_enabled(&self.staticcheck, "SA4006")
                || staticcheck_check_enabled(&self.staticcheck, "SA4031"),
        );
        bag.insert("revive", self.revive.to_guff_revive());
        bag.insert("dupl", self.dupl.to_guff_dupl());
        bag.insert("misspell", self.misspell.to_guff_misspell());
        bag.insert("gocyclo", self.gocyclo.to_guff_gocyclo());
        bag.insert("maintidx", self.maintidx.to_guff_maintidx());
        bag.insert("gocognit", self.gocognit.to_guff_gocognit());
        bag.insert("nestif", self.nestif.to_guff_nestif());
        bag.insert("dogsled", self.dogsled.to_guff_dogsled());
        bag.insert("funlen", self.funlen.to_guff_funlen());
        bag.insert("cyclop", self.cyclop.to_guff_cyclop());
        bag.insert("lll", self.lll.to_guff_lll());
        bag.insert("nakedret", self.nakedret.to_guff_nakedret());
        bag.insert("nlreturn", self.nlreturn.to_guff_nlreturn());
        bag.insert("predeclared", self.predeclared.to_guff_predeclared());
        bag.insert("whitespace", self.whitespace.to_guff_whitespace());
        bag.insert("mnd", self.mnd.to_guff_mnd());
        bag.insert("prealloc", self.prealloc.to_guff_prealloc());
        bag.insert("tagalign", self.tagalign.to_guff_tagalign());
        bag.insert("wsl", self.wsl.to_guff_wsl());
        bag.insert("perfsprint", self.perfsprint.to_guff_perfsprint());
        bag.insert("goconst", self.goconst.to_guff_goconst());
        bag.insert("copyloopvar", self.copyloopvar.to_guff_copyloopvar());
        bag.insert("usetesting", self.usetesting.to_guff_usetesting());
        bag.insert("usestdlibvars", self.usestdlibvars.to_guff_usestdlibvars());
        bag.insert("unconvert", self.unconvert.to_guff_unconvert());
        bag.insert("exhaustruct", self.exhaustruct.to_guff_exhaustruct());
        bag.insert("exhaustive", self.exhaustive.to_guff_exhaustive());
        bag.insert("musttag", self.musttag.to_guff_musttag());
        bag.insert("loggercheck", self.loggercheck.to_guff_loggercheck());
        bag.insert("sloglint", self.sloglint.to_guff_sloglint());
        bag.insert("testifylint", self.testifylint.to_guff_testifylint());
        bag.insert("errchkjson", self.errchkjson.to_guff_errchkjson());
        bag.insert("wrapcheck", self.wrapcheck.to_guff_wrapcheck());
        bag.insert("rowserrcheck", self.rowserrcheck.to_guff_rowserrcheck());
        bag.insert("bodyclose", self.bodyclose.to_guff_bodyclose());
        bag.insert("godot", self.godot.to_guff_godot());
        bag.insert("godox", self.godox.to_guff_godox());
        bag.insert("dupword", self.dupword.to_guff_dupword());
        bag.insert("godoclint", self.godoclint.to_guff_godoclint());
        bag.insert("depguard", self.depguard.to_guff_depguard());
        bag.insert(
            "gomoddirectives",
            self.gomoddirectives.to_guff_gomoddirectives(),
        );
        bag.insert("gomodguard", self.gomodguard.to_guff_gomodguard());
        {
            let modernize = self.modernize.to_guff_modernize();
            // Runner uses this to skip modernize's import-fact fan-out when `newexpr`
            // is disabled (the only check that imports/exports NewLikeFact). Without
            // it, modernize still schedules on every imported package (~1000+ actions
            // on prometheus) even though those facts are never consumed.
            bag.insert(
                "modernize_schedule_facts",
                !modernize.disable.iter().any(|d| d == "newexpr"),
            );
            bag.insert("modernize", modernize);
        }
        bag.insert("gocritic", self.gocritic.to_guff_gocritic());
        bag.insert("forbidigo", self.forbidigo.to_guff_forbidigo());
        bag.insert("bidichk", self.bidichk.to_guff_bidichk());
        bag.insert("gosmopolitan", self.gosmopolitan.to_guff_gosmopolitan());
        bag.insert("goheader", self.goheader.to_guff_goheader());
        bag.insert("asasalint", self.asasalint.to_guff_asasalint());
        bag.insert("reassign", self.reassign.to_guff_reassign());
        bag.insert("recvcheck", self.recvcheck.to_guff_recvcheck());
        bag.insert("thelper", self.thelper.to_guff_thelper());
        bag.insert("iface", self.iface.to_guff_iface());
        bag.insert("importas", self.importas.to_guff_importas());
        bag.insert(
            "interfacebloat",
            self.interfacebloat.to_guff_interfacebloat(),
        );
        bag.insert(
            "embeddedstructfieldcheck",
            self.embeddedstructfieldcheck
                .to_guff_embeddedstructfieldcheck(),
        );
        bag.insert(
            "gochecksumtype",
            self.gochecksumtype.to_guff_gochecksumtype(),
        );
        bag.insert("inamedparam", self.inamedparam.to_guff_inamedparam());
        bag.insert(
            "nonamedreturns",
            self.nonamedreturns.to_guff_nonamedreturns(),
        );
        bag.insert("funcorder", self.funcorder.to_guff_funcorder());
        bag.insert("varnamelen", self.varnamelen.to_guff_varnamelen());
        bag.insert("unparam", self.unparam.to_guff_unparam());
        bag.insert("unqueryvet", self.unqueryvet.to_guff_unqueryvet());
        bag.insert("promlinter", self.promlinter.to_guff_promlinter());
        bag.insert("ginkgolinter", self.ginkgolinter.to_guff_ginkgolinter());
        bag.insert("wsl_v5", self.wsl_v5.to_guff_wsl_v5());
        bag.insert("paralleltest", self.paralleltest.to_guff_paralleltest());
        bag.insert("testpackage", self.testpackage.to_guff_testpackage());
        bag.insert("tagliatelle", self.tagliatelle.to_guff_tagliatelle());
        bag.insert("decorder", self.decorder.to_guff_decorder());
        bag.insert("iotamixing", self.iotamixing.to_guff_iotamixing());
        bag.insert("grouper", self.grouper.to_guff_grouper());
        bag.insert("ireturn", self.ireturn.to_guff_ireturn());
        bag.insert("gosec", self.gosec.to_guff_gosec());
        for (name, cfg) in &self.custom {
            if !cfg.type_.is_empty() && cfg.type_ != "module" {
                eprintln!(
                    "guff: linters.settings.custom.{name}: type {:?} is not supported (only \"module\")",
                    cfg.type_
                );
                continue;
            }
            // Nested settings for Pass-time `pass.settings::<Value>(name)`.
            bag.insert(name.clone(), cfg.settings.clone());
        }
        Arc::new(bag)
    }

    /// Filter / select analyzers for a single golangci linter name.
    pub fn apply_to_analyzers(
        &self,
        linter: &str,
        analyzers: Vec<&'static Analyzer>,
    ) -> Vec<&'static Analyzer> {
        match linter {
            "govet" => filter_govet(&self.govet, analyzers),
            "staticcheck" => filter_staticcheck(&self.staticcheck, analyzers),
            _ => analyzers,
        }
    }
}

fn filter_govet(
    settings: &GovetSettings,
    analyzers: Vec<&'static Analyzer>,
) -> Vec<&'static Analyzer> {
    if !settings.enable_all
        && !settings.disable_all
        && settings.enable.is_empty()
        && settings.disable.is_empty()
    {
        return analyzers;
    }

    let enable: HashSet<&str> = settings.enable.iter().map(String::as_str).collect();
    let disable: HashSet<&str> = settings.disable.iter().map(String::as_str).collect();

    // guff-govet::analyzers() is the full available set (= golangci default ∪ extras).
    if settings.disable_all {
        return analyzers
            .into_iter()
            .filter(|a| enable.contains(a.name))
            .collect();
    }

    // Default / enable-all: full registry minus `disable`.
    // `enable` only matters with disable-all (above); otherwise the registry
    // already includes every pass enable could add.
    let _ = (settings.enable_all, &enable);
    analyzers
        .into_iter()
        .filter(|a| !disable.contains(a.name))
        .collect()
}

fn filter_staticcheck(
    settings: &StaticcheckSettings,
    analyzers: Vec<&'static Analyzer>,
) -> Vec<&'static Analyzer> {
    // Upstream default (staticcheck.io): all checks except opinionated ST*/SA9003.
    // When `checks` is unset, apply the same filter so ST1000 etc. stay off.
    const DEFAULT_DISABLED: &[&str] = &[
        "SA9003", "ST1000", "ST1003", "ST1016", "ST1020", "ST1021", "ST1022",
    ];

    let Some(checks) = settings.checks.as_ref() else {
        return analyzers
            .into_iter()
            .filter(|a| !DEFAULT_DISABLED.contains(&a.name))
            .collect();
    };
    if checks.is_empty() {
        return analyzers
            .into_iter()
            .filter(|a| !DEFAULT_DISABLED.contains(&a.name))
            .collect();
    }

    let mut allow_all = false;
    let mut enabled: HashSet<String> = HashSet::new();
    let mut disabled: HashSet<String> = HashSet::new();
    let mut disabled_prefixes: Vec<String> = Vec::new();

    for c in checks {
        if c == "all" {
            allow_all = true;
            continue;
        }
        if let Some(rest) = c.strip_prefix('-') {
            if let Some(prefix) = rest.strip_suffix('*') {
                // golangci: `-QF*` / `-ST*` disable a whole family.
                disabled_prefixes.push(prefix.to_string());
            } else {
                disabled.insert(rest.to_string());
            }
        } else {
            enabled.insert(c.clone());
        }
    }

    analyzers
        .into_iter()
        .filter(|a| {
            if disabled.contains(a.name) {
                return false;
            }
            if disabled_prefixes.iter().any(|p| a.name.starts_with(p.as_str())) {
                return false;
            }
            if allow_all {
                return true;
            }
            if enabled.is_empty() {
                return true;
            }
            enabled.contains(a.name)
        })
        .collect()
}

/// Whether a named staticcheck analyzer would survive [`filter_staticcheck`].
fn staticcheck_check_enabled(settings: &StaticcheckSettings, name: &str) -> bool {
    const DEFAULT_DISABLED: &[&str] = &[
        "SA9003", "ST1000", "ST1003", "ST1016", "ST1020", "ST1021", "ST1022",
    ];
    let Some(checks) = settings.checks.as_ref() else {
        return !DEFAULT_DISABLED.contains(&name);
    };
    if checks.is_empty() {
        return !DEFAULT_DISABLED.contains(&name);
    }
    let mut allow_all = false;
    let mut enabled = false;
    let mut disabled = false;
    let mut any_positive = false;
    for c in checks {
        if c == "all" {
            allow_all = true;
            continue;
        }
        if let Some(rest) = c.strip_prefix('-') {
            if rest == name {
                disabled = true;
            } else if let Some(prefix) = rest.strip_suffix('*') {
                if name.starts_with(prefix) {
                    disabled = true;
                }
            }
        } else {
            any_positive = true;
            if c == name {
                enabled = true;
            }
        }
    }
    if disabled {
        return false;
    }
    if allow_all {
        return true;
    }
    if !any_positive {
        return true;
    }
    enabled
}

impl ReviveSettings {
    /// True when no revive settings were customized — matches golangci-lint's
    /// `reflect.DeepEqual(cfg, zero)` path that keeps revive's default rule set.
    fn is_zero_like(&self) -> bool {
        self.severity.is_none()
            && self.rules.is_none()
            && self.confidence.is_none()
            && !self.ignore_generated_header
            && !self.enable_default_rules
            && !self.enable_all_rules
    }

    pub fn to_guff_revive(&self) -> guff_revive::Settings {
        let mapped_rules = self.rules.as_ref().map(|rules| {
            rules
                .iter()
                .map(|rule| guff_revive::RuleSetting {
                    name: rule.name.clone(),
                    arguments: rule
                        .arguments
                        .iter()
                        .map(convert_revive_argument)
                        .collect(),
                    disabled: rule.disabled,
                    severity: rule.severity.clone(),
                })
                .collect()
        });
        // golangci-lint: any non-zero revive settings without
        // `enable-default-rules` / `enable-all-rules` / explicit `rules`
        // starts from an empty rule set (see golinters/revive getConfig).
        // guff previously treated `rules: None` as "golint defaults" even when
        // confidence/severity were set, which flooded findings vs golangci.
        let rules = match mapped_rules {
            Some(rules) => Some(rules),
            None if self.enable_default_rules || self.enable_all_rules || self.is_zero_like() => {
                None
            }
            None => Some(Vec::new()),
        };
        guff_revive::Settings {
            severity: self.severity.clone(),
            rules,
            confidence: self.confidence,
            ignore_generated_header: self.ignore_generated_header,
            enable_default_rules: self.enable_default_rules,
            enable_all_rules: self.enable_all_rules,
        }
    }
}

impl DuplSettings {
    pub fn to_guff_dupl(&self) -> guff_dupl::Options {
        guff_dupl::Options {
            threshold: self.threshold.unwrap_or(guff_dupl::DEFAULT_THRESHOLD),
        }
    }
}

impl MisspellSettings {
    pub fn to_guff_misspell(&self) -> guff_misspell::Options {
        guff_misspell::Options {
            locale: self.locale.clone().unwrap_or_default(),
            ignore_words: self.ignore_words.clone(),
            extra_words: self
                .extra_words
                .iter()
                .map(|w| guff_misspell::ExtraWord {
                    typo: w.typo.clone(),
                    correction: w.correction.clone(),
                })
                .collect(),
            mode: self.mode.clone().unwrap_or_default(),
        }
    }
}

impl GocycloSettings {
    pub fn to_guff_gocyclo(&self) -> guff_style::GocycloOptions {
        let defaults = guff_style::GocycloOptions::default();
        guff_style::GocycloOptions {
            min_complexity: self.min_complexity.unwrap_or(defaults.min_complexity),
        }
    }
}

impl GocognitSettings {
    pub fn to_guff_gocognit(&self) -> guff_style::GocognitOptions {
        let defaults = guff_style::GocognitOptions::default();
        guff_style::GocognitOptions {
            min_complexity: self.min_complexity.unwrap_or(defaults.min_complexity),
        }
    }
}

impl NestifSettings {
    pub fn to_guff_nestif(&self) -> guff_style::NestifOptions {
        let defaults = guff_style::NestifOptions::default();
        guff_style::NestifOptions {
            min_complexity: self.min_complexity.unwrap_or(defaults.min_complexity),
        }
    }
}

impl DogsledSettings {
    pub fn to_guff_dogsled(&self) -> guff_style::DogsledOptions {
        let defaults = guff_style::DogsledOptions::default();
        guff_style::DogsledOptions {
            max_blank_identifiers: self
                .max_blank_identifiers
                .unwrap_or(defaults.max_blank_identifiers),
        }
    }
}

impl FunlenSettings {
    pub fn to_guff_funlen(&self) -> guff_style::FunlenOptions {
        let defaults = guff_style::FunlenOptions::default();
        guff_style::FunlenOptions {
            // ≤0 disables that limit (ultraware/funlen + golangci convention).
            lines: match self.lines {
                Some(n) if n <= 0 => usize::MAX,
                Some(n) => n as usize,
                None => defaults.lines,
            },
            statements: match self.statements {
                Some(n) if n <= 0 => usize::MAX,
                Some(n) => n as usize,
                None => defaults.statements,
            },
            ignore_comments: self.ignore_comments.unwrap_or(defaults.ignore_comments),
        }
    }
}

impl CyclopSettings {
    pub fn to_guff_cyclop(&self) -> guff_style::CyclopOptions {
        let defaults = guff_style::CyclopOptions::default();
        guff_style::CyclopOptions {
            max_complexity: self.max_complexity.unwrap_or(defaults.max_complexity),
            package_average: self.package_average.unwrap_or(defaults.package_average),
            skip_tests: self.skip_tests.unwrap_or(defaults.skip_tests),
        }
    }
}

impl LllSettings {
    pub fn to_guff_lll(&self) -> guff_style::LllOptions {
        let defaults = guff_style::LllOptions::default();
        guff_style::LllOptions {
            line_length: self.line_length.unwrap_or(defaults.line_length),
            tab_width: self.tab_width.unwrap_or(defaults.tab_width),
        }
    }
}

impl NakedretSettings {
    pub fn to_guff_nakedret(&self) -> guff_style::NakedretOptions {
        let defaults = guff_style::NakedretOptions::default();
        guff_style::NakedretOptions {
            max_func_lines: self.max_func_lines.unwrap_or(defaults.max_func_lines),
            skip_test_files: self.skip_test_files.unwrap_or(defaults.skip_test_files),
        }
    }
}

impl NlreturnSettings {
    pub fn to_guff_nlreturn(&self) -> guff_style::NlreturnOptions {
        let defaults = guff_style::NlreturnOptions::default();
        guff_style::NlreturnOptions {
            block_size: self.block_size.unwrap_or(defaults.block_size),
        }
    }
}

impl PredeclaredSettings {
    pub fn to_guff_predeclared(&self) -> guff_style::PredeclaredOptions {
        let defaults = guff_style::PredeclaredOptions::default();
        guff_style::PredeclaredOptions {
            ignore: if self.ignore.is_empty() {
                defaults.ignore
            } else {
                self.ignore.clone()
            },
            qualified: self.qualified.unwrap_or(defaults.qualified),
        }
    }
}

impl WhitespaceSettings {
    pub fn to_guff_whitespace(&self) -> guff_style::WhitespaceOptions {
        let defaults = guff_style::WhitespaceOptions::default();
        guff_style::WhitespaceOptions {
            multi_if: self.multi_if.unwrap_or(defaults.multi_if),
            multi_func: self.multi_func.unwrap_or(defaults.multi_func),
        }
    }
}

impl MndSettings {
    pub fn to_guff_mnd(&self) -> guff_style::MndOptions {
        let defaults = guff_style::MndOptions::default();
        guff_style::MndOptions {
            checks: self.checks.clone().unwrap_or(defaults.checks),
            ignored_numbers: if self.ignored_numbers.is_empty() {
                defaults.ignored_numbers
            } else {
                self.ignored_numbers.clone()
            },
            ignored_files: self.ignored_files.clone(),
            ignored_functions: if self.ignored_functions.is_empty() {
                defaults.ignored_functions
            } else {
                self.ignored_functions.clone()
            },
        }
    }
}

impl PreallocSettings {
    pub fn to_guff_prealloc(&self) -> guff_style::PreallocOptions {
        let defaults = guff_style::PreallocOptions::default();
        guff_style::PreallocOptions {
            simple: self.simple.unwrap_or(defaults.simple),
            range_loops: self.range_loops.unwrap_or(defaults.range_loops),
            for_loops: self.for_loops.unwrap_or(defaults.for_loops),
        }
    }
}

impl TagalignSettings {
    pub fn to_guff_tagalign(&self) -> guff_style::TagalignOptions {
        let defaults = guff_style::TagalignOptions::default();
        guff_style::TagalignOptions {
            align: self.align.unwrap_or(defaults.align),
            sort: self.sort.unwrap_or(defaults.sort),
            order: if self.order.is_empty() {
                defaults.order
            } else {
                self.order.clone()
            },
            strict: self.strict.unwrap_or(defaults.strict),
        }
    }
}

impl WslSettings {
    pub fn to_guff_wsl(&self) -> guff_style::WslOptions {
        let defaults = guff_style::WslOptions::default();
        guff_style::WslOptions {
            strict_append: self.strict_append.unwrap_or(defaults.strict_append),
            allow_assign_and_call: self
                .allow_assign_and_call
                .unwrap_or(defaults.allow_assign_and_call),
            allow_assign_and_anything: self
                .allow_assign_and_anything
                .unwrap_or(defaults.allow_assign_and_anything),
            allow_multiline_assign: self
                .allow_multiline_assign
                .unwrap_or(defaults.allow_multiline_assign),
            allow_cuddle_with_calls: if self.allow_cuddle_with_calls.is_empty() {
                defaults.allow_cuddle_with_calls
            } else {
                self.allow_cuddle_with_calls.clone()
            },
            allow_cuddle_with_rhs: if self.allow_cuddle_with_rhs.is_empty() {
                defaults.allow_cuddle_with_rhs
            } else {
                self.allow_cuddle_with_rhs.clone()
            },
        }
    }
}

impl PerfsprintSettings {
    pub fn to_guff_perfsprint(&self) -> guff_style::PerfsprintOptions {
        let defaults = guff_style::PerfsprintOptions::default();
        guff_style::PerfsprintOptions {
            integer_format: self.integer_format.unwrap_or(defaults.integer_format),
            int_conversion: self.int_conversion.unwrap_or(defaults.int_conversion),
            error_format: self.error_format.unwrap_or(defaults.error_format),
            err_error: self.err_error.unwrap_or(defaults.err_error),
            errorf: self.errorf.unwrap_or(defaults.errorf),
            string_format: self.string_format.unwrap_or(defaults.string_format),
            sprintf1: self.sprintf1.unwrap_or(defaults.sprintf1),
            strconcat: self.strconcat.unwrap_or(defaults.strconcat),
            bool_format: self.bool_format.unwrap_or(defaults.bool_format),
            hex_format: self.hex_format.unwrap_or(defaults.hex_format),
            concat_loop: self.concat_loop.unwrap_or(defaults.concat_loop),
            loop_other_ops: self.loop_other_ops.unwrap_or(defaults.loop_other_ops),
        }
    }
}

impl GoconstSettings {
    pub fn to_guff_goconst(&self) -> guff_style::GoconstOptions {
        let defaults = guff_style::GoconstOptions::default();
        let mut ignore_strings = self.ignore_string_values.clone();
        // golangci migrates deprecated `ignore-strings` into the list form.
        if let Some(legacy) = self.ignore_strings.as_ref() {
            if !legacy.is_empty() && !ignore_strings.iter().any(|p| p == legacy) {
                ignore_strings.push(legacy.clone());
            }
        }
        guff_style::GoconstOptions {
            min_len: self.min_len.unwrap_or(defaults.min_len),
            min_occurrences: self.min_occurrences.unwrap_or(defaults.min_occurrences),
            ignore_calls: self.ignore_calls.unwrap_or(defaults.ignore_calls),
            ignore_tests: self.ignore_tests.unwrap_or(defaults.ignore_tests),
            match_constant: self.match_constant.unwrap_or(defaults.match_constant),
            find_duplicates: self.find_duplicates.unwrap_or(defaults.find_duplicates),
            numbers: self.numbers.unwrap_or(defaults.numbers),
            number_min: self.min.unwrap_or(defaults.number_min),
            number_max: self.max.unwrap_or(defaults.number_max),
            ignore_strings,
        }
    }
}

impl CopyloopvarSettings {
    pub fn to_guff_copyloopvar(&self) -> guff_style::CopyloopvarOptions {
        let defaults = guff_style::CopyloopvarOptions::default();
        guff_style::CopyloopvarOptions {
            check_alias: self.check_alias.unwrap_or(defaults.check_alias),
        }
    }
}

impl UnconvertSettings {
    pub fn to_guff_unconvert(&self) -> guff_style::UnconvertOptions {
        let defaults = guff_style::UnconvertOptions::default();
        guff_style::UnconvertOptions {
            fast_math: self.fast_math.unwrap_or(defaults.fast_math),
            safe: self.safe.unwrap_or(defaults.safe),
        }
    }
}

impl ExhaustructSettings {
    pub fn to_guff_exhaustruct(&self) -> guff_style::ExhaustructOptions {
        let defaults = guff_style::ExhaustructOptions::default();
        guff_style::ExhaustructOptions {
            include: self.include.clone(),
            exclude: self.exclude.clone(),
            allow_empty: self.allow_empty.unwrap_or(defaults.allow_empty),
            allow_empty_rx: self.allow_empty_rx.clone(),
            allow_empty_returns: self
                .allow_empty_returns
                .unwrap_or(defaults.allow_empty_returns),
            allow_empty_declarations: self
                .allow_empty_declarations
                .unwrap_or(defaults.allow_empty_declarations),
        }
    }
}

impl ExhaustiveSettings {
    pub fn to_guff_exhaustive(&self) -> guff_style::ExhaustiveOptions {
        let defaults = guff_style::ExhaustiveOptions::default();
        let (check_switch, check_map) = if self.check.is_empty() {
            (defaults.check_switch, defaults.check_map)
        } else {
            let switch = self.check.iter().any(|c| c == "switch");
            let map = self.check.iter().any(|c| c == "map");
            (switch, map)
        };
        guff_style::ExhaustiveOptions {
            check_switch,
            check_map,
            default_signifies_exhaustive: self
                .default_signifies_exhaustive
                .unwrap_or(defaults.default_signifies_exhaustive),
            default_case_required: self
                .default_case_required
                .unwrap_or(defaults.default_case_required),
            ignore_enum_members: self
                .ignore_enum_members
                .clone()
                .unwrap_or(defaults.ignore_enum_members),
            ignore_enum_types: self
                .ignore_enum_types
                .clone()
                .unwrap_or(defaults.ignore_enum_types),
            package_scope_only: self
                .package_scope_only
                .unwrap_or(defaults.package_scope_only),
        }
    }
}

impl MusttagSettings {
    pub fn to_guff_musttag(&self) -> guff_style::MusttagOptions {
        guff_style::MusttagOptions {
            functions: self
                .functions
                .iter()
                .filter(|f| !f.name.is_empty() && !f.tag.is_empty())
                .map(|f| guff_style::MusttagFunc {
                    name: f.name.clone(),
                    tag: f.tag.clone(),
                    arg_pos: f.arg_pos,
                })
                .collect(),
        }
    }
}

impl LoggercheckSettings {
    pub fn to_guff_loggercheck(&self) -> guff_style::LoggercheckOptions {
        let defaults = guff_style::LoggercheckOptions::default();
        guff_style::LoggercheckOptions {
            kitlog: self.kitlog.unwrap_or(defaults.kitlog),
            klog: self.klog.unwrap_or(defaults.klog),
            logr: self.logr.unwrap_or(defaults.logr),
            slog: self.slog.unwrap_or(defaults.slog),
            zap: self.zap.unwrap_or(defaults.zap),
            require_string_key: self.require_string_key,
            no_printf_like: self.no_printf_like,
            rules: self.rules.clone(),
        }
    }
}

impl SloglintSettings {
    pub fn to_guff_sloglint(&self) -> guff_style::SloglintOptions {
        guff_style::SloglintOptions {
            no_mixed_args: self.no_mixed_args,
            kv_only: self.kv_only,
            attr_only: self.attr_only,
            no_global: self.no_global.clone().filter(|s| !s.is_empty()),
            context: self.context.clone().filter(|s| !s.is_empty()),
            static_msg: self.static_msg,
            msg_style: self.msg_style.clone().filter(|s| !s.is_empty()),
            no_raw_keys: self.no_raw_keys,
            key_naming_case: self.key_naming_case.clone().filter(|s| !s.is_empty()),
            allowed_keys: self.allowed_keys.clone(),
            forbidden_keys: self.forbidden_keys.clone(),
            args_on_sep_lines: self.args_on_sep_lines,
            custom_funcs: self
                .custom_funcs
                .iter()
                .map(|f| guff_style::SloglintFunc {
                    name: f.name.clone(),
                    msg_pos: f.msg_pos,
                    args_pos: f.args_pos,
                })
                .collect(),
        }
    }
}

impl TestifylintSettings {
    pub fn to_guff_testifylint(&self) -> guff_style::TestifylintOptions {
        let suite_mode = match self
            .suite_extra_assert_call
            .mode
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("require") => guff_style::SuiteExtraAssertCallMode::Require,
            _ => guff_style::SuiteExtraAssertCallMode::Remove,
        };
        guff_style::TestifylintOptions {
            enable_all: self.enable_all,
            disable_all: self.disable_all,
            enable: self.enable.clone(),
            disable: self.disable.clone(),
            bool_compare_ignore_custom_types: self.bool_compare.ignore_custom_types,
            expected_actual_pattern: self.expected_actual.pattern.clone(),
            time_compare_suppress_calls_pattern: self.time_compare.suppress_calls_pattern.clone(),
            formatter_check_format_string: self.formatter.check_format_string,
            formatter_require_f_funcs: self.formatter.require_f_funcs,
            formatter_require_string_msg: self.formatter.require_string_msg,
            suite_extra_assert_call_mode: suite_mode,
            require_error_fn_pattern: self.require_error.fn_pattern.clone(),
            go_require_ignore_http_handlers: self.go_require.ignore_http_handlers,
        }
    }
}

impl UsetestingSettings {
    pub fn to_guff_usetesting(&self) -> guff_style::UsetestingOptions {
        let defaults = guff_style::UsetestingOptions::default();
        guff_style::UsetestingOptions {
            os_create_temp: self.os_create_temp.unwrap_or(defaults.os_create_temp),
            os_mkdir_temp: self.os_mkdir_temp.unwrap_or(defaults.os_mkdir_temp),
            os_setenv: self.os_setenv.unwrap_or(defaults.os_setenv),
            os_temp_dir: self.os_temp_dir.unwrap_or(defaults.os_temp_dir),
            os_chdir: self.os_chdir.unwrap_or(defaults.os_chdir),
            context_background: self
                .context_background
                .unwrap_or(defaults.context_background),
            context_todo: self.context_todo.unwrap_or(defaults.context_todo),
        }
    }
}

impl UsestdlibvarsSettings {
    pub fn to_guff_usestdlibvars(&self) -> guff_style::UsestdlibvarsOptions {
        let defaults = guff_style::UsestdlibvarsOptions::default();
        guff_style::UsestdlibvarsOptions {
            http_method: self.http_method.unwrap_or(defaults.http_method),
            http_status_code: self
                .http_status_code
                .unwrap_or(defaults.http_status_code),
            time_weekday: self.time_weekday.unwrap_or(defaults.time_weekday),
            time_month: self.time_month.unwrap_or(defaults.time_month),
            time_layout: self.time_layout.unwrap_or(defaults.time_layout),
            crypto_hash: self.crypto_hash.unwrap_or(defaults.crypto_hash),
            default_rpc_path: self
                .default_rpc_path
                .unwrap_or(defaults.default_rpc_path),
            sql_isolation_level: self
                .sql_isolation_level
                .unwrap_or(defaults.sql_isolation_level),
            tls_signature_scheme: self
                .tls_signature_scheme
                .unwrap_or(defaults.tls_signature_scheme),
            constant_kind: self.constant_kind.unwrap_or(defaults.constant_kind),
            time_date_month: self
                .time_date_month
                .unwrap_or(defaults.time_date_month),
        }
    }
}

impl ErrchkjsonSettings {
    pub fn to_guff_errchkjson(&self) -> guff_error::ErrchkjsonOptions {
        // golangci: omit-safe = !check-error-free-encoding
        guff_error::ErrchkjsonOptions {
            omit_safe: !self.check_error_free_encoding,
            report_no_exported: self.report_no_exported,
        }
    }
}

impl WrapcheckSettings {
    pub fn to_guff_wrapcheck(&self) -> guff_error::WrapcheckOptions {
        guff_error::WrapcheckOptions {
            ignore_sigs: self.ignore_sigs.clone(),
            extra_ignore_sigs: self.extra_ignore_sigs.clone(),
            ignore_sig_regexps: self.ignore_sig_regexps.clone(),
            ignore_package_globs: self.ignore_package_globs.clone(),
            ignore_interface_regexps: self.ignore_interface_regexps.clone(),
            report_internal_errors: self.report_internal_errors,
        }
    }
}

impl RowserrcheckSettings {
    pub fn to_guff_rowserrcheck(&self) -> guff_error::RowserrcheckOptions {
        guff_error::RowserrcheckOptions {
            packages: self.packages.clone(),
        }
    }
}

impl BodycloseSettings {
    pub fn to_guff_bodyclose(&self) -> guff_context::BodycloseOptions {
        guff_context::BodycloseOptions {
            check_consumption: self.check_consumption,
        }
    }
}

impl GodotSettings {
    pub fn to_guff_godot(&self) -> guff_comment::GodotOptions {
        let defaults = guff_comment::GodotOptions::default();
        guff_comment::GodotOptions {
            scope: self.scope.clone().unwrap_or(defaults.scope),
            exclude: self.exclude.clone(),
            period: self.period.unwrap_or(defaults.period),
            capital: self.capital.unwrap_or(defaults.capital),
        }
    }
}

impl GodoxSettings {
    pub fn to_guff_godox(&self) -> guff_comment::GodoxOptions {
        guff_comment::GodoxOptions {
            keywords: self.keywords.clone(),
        }
    }
}

impl DupwordSettings {
    pub fn to_guff_dupword(&self) -> guff_comment::DupwordOptions {
        guff_comment::DupwordOptions {
            keywords: self.keywords.clone(),
            ignore: self.ignore.clone(),
            comments_only: self.comments_only.unwrap_or(false),
        }
    }
}

impl GodoclintSettings {
    pub fn to_guff_godoclint(&self) -> guff_comment::GodoclintOptions {
        let defaults = guff_comment::GodoclintOptions::default();
        guff_comment::GodoclintOptions {
            default: self
                .default_set
                .clone()
                .unwrap_or(defaults.default),
            enable: self.enable.clone(),
            disable: self.disable.clone(),
        }
    }
}

impl DepguardSettings {
    pub fn to_guff_depguard(&self) -> guff_import::DepguardOptions {
        let mut rules = Vec::new();
        for (name, rule) in &self.rules {
            rules.push(guff_import::DepguardRule {
                name: name.clone(),
                list_mode: guff_import::ListMode::parse(
                    rule.list_mode.as_deref().unwrap_or("original"),
                ),
                files: rule.files.clone(),
                allow: rule.allow.clone(),
                deny: rule
                    .deny
                    .iter()
                    .map(|d| guff_import::DenyEntry {
                        pkg: d.pkg.clone(),
                        desc: d.desc.clone(),
                    })
                    .collect(),
            });
        }
        guff_import::DepguardOptions { rules }
    }
}

impl GomoddirectivesSettings {
    pub fn to_guff_gomoddirectives(&self) -> guff_import::GomoddirectivesOptions {
        guff_import::GomoddirectivesOptions {
            replace_local: self.replace_local,
            replace_allow_list: self.replace_allow_list.clone(),
            retract_allow_no_explanation: self.retract_allow_no_explanation,
            exclude_forbidden: self.exclude_forbidden,
            toolchain_forbidden: self.toolchain_forbidden,
            tool_forbidden: self.tool_forbidden,
            go_debug_forbidden: self.go_debug_forbidden,
        }
    }
}

impl GomodguardSettings {
    pub fn to_guff_gomodguard(&self) -> guff_import::GomodguardOptions {
        guff_import::GomodguardOptions {
            blocked_modules: self.blocked_modules.clone(),
            local_replace_directives: self.local_replace_directives,
        }
    }
}

impl ModernizeSettings {
    pub fn to_guff_modernize(&self) -> guff_style::ModernizeOptions {
        // Checkers guff implements that golangci-lint's vendored x/tools Suite
        // (v0.44 as of golangci-lint 2.12) does not enable. Disable them by
        // default so finding sets match golangci under the same config.
        // User `disable:` entries remain additive; there is no Suite-extra
        // re-enable knob yet (golangci only exposes `disable`).
        const SUITE_EXTRA_OFF: &[&str] = &[
            "errorsastype",      // after v0.44
            "slicesdelete",      // commented out upstream (not nil-preserving)
            "bloop",             // commented out upstream
            "importcomment",     // not in Suite
            "reflecttypeassert", // not in Suite
            // Guff's path is Split/SplitN→Cut; Suite stringscut is Index→Cut
            // only (SplitN support landed after v0.44).
            "stringscut",
        ];
        let mut disable = self.disable.clone();
        for name in SUITE_EXTRA_OFF {
            if !disable.iter().any(|d| d == *name) {
                disable.push((*name).to_string());
            }
        }
        guff_style::ModernizeOptions { disable }
    }
}

impl GocriticSettings {
    pub fn to_guff_gocritic(&self) -> guff_style::GocriticOptions {
        let mut check_settings = guff_style::GocriticCheckSettings::default();
        if let Some(settings) = &self.settings {
            if let Some(n) = gocritic_param_usize(settings, "tooManyResultsChecker", "maxResults") {
                check_settings.too_many_results_max = n;
            }
            if let Some(n) = gocritic_param_usize(settings, "ifElseChain", "minThreshold") {
                check_settings.if_else_chain_min_threshold = n;
            }
            if let Some(b) = gocritic_param_bool(settings, "unnamedResult", "checkExported") {
                check_settings.unnamed_result_check_exported = b;
            }
        }
        guff_style::GocriticOptions {
            enable_all: self.enable_all,
            disable_all: self.disable_all,
            enabled_checks: self.enabled_checks.clone(),
            disabled_checks: self.disabled_checks.clone(),
            enabled_tags: self.enabled_tags.clone(),
            disabled_tags: self.disabled_tags.clone(),
            check_settings,
        }
    }
}

impl ForbidigoSettings {
    pub fn to_guff_forbidigo(&self) -> guff_style::ForbidigoOptions {
        let defaults = guff_style::ForbidigoOptions::default();
        let forbid = self
            .forbid
            .iter()
            .map(|e| match e {
                ForbidigoForbidEntry::Pattern(p) => guff_style::ForbidigoPattern {
                    pattern: p.clone(),
                    pkg: String::new(),
                    msg: String::new(),
                },
                ForbidigoForbidEntry::Struct(s) => guff_style::ForbidigoPattern {
                    pattern: s.pattern.clone(),
                    pkg: s.pkg.clone(),
                    msg: s.msg.clone(),
                },
            })
            .collect();
        guff_style::ForbidigoOptions {
            forbid,
            exclude_godoc_examples: self
                .exclude_godoc_examples
                .unwrap_or(defaults.exclude_godoc_examples),
            analyze_types: self.analyze_types,
        }
    }
}

impl ImportasSettings {
    pub fn to_guff_importas(&self) -> guff_import::ImportasOptions {
        guff_import::ImportasOptions {
            alias: self
                .alias
                .iter()
                .map(|a| guff_import::ImportasAlias {
                    pkg: a.pkg.clone(),
                    alias: a.alias.clone(),
                })
                .collect(),
            no_unaliased: self.no_unaliased,
            no_extra_aliases: self.no_extra_aliases,
        }
    }
}

impl AsasalintSettings {
    pub fn to_guff_asasalint(&self) -> guff_style::AsasalintOptions {
        guff_style::AsasalintOptions {
            exclude: self.exclude.clone(),
            use_builtin_exclusions: self.use_builtin_exclusions,
        }
    }
}

impl ReassignSettings {
    pub fn to_guff_reassign(&self) -> guff_style::ReassignOptions {
        guff_style::ReassignOptions {
            patterns: self.patterns.clone(),
        }
    }
}

impl RecvcheckSettings {
    pub fn to_guff_recvcheck(&self) -> guff_style::RecvcheckOptions {
        guff_style::RecvcheckOptions {
            disable_builtin: self.disable_builtin,
            exclusions: self.exclusions.clone(),
        }
    }
}

impl ThelperKindSettings {
    fn to_guff(&self) -> guff_style::ThelperKindOptions {
        let defaults = guff_style::ThelperKindOptions::default();
        guff_style::ThelperKindOptions {
            first: self.first.unwrap_or(defaults.first),
            name: self.name.unwrap_or(defaults.name),
            begin: self.begin.unwrap_or(defaults.begin),
        }
    }
}

impl ThelperSettings {
    pub fn to_guff_thelper(&self) -> guff_style::ThelperOptions {
        guff_style::ThelperOptions {
            test: self.test.to_guff(),
            fuzz: self.fuzz.to_guff(),
            benchmark: self.benchmark.to_guff(),
            tb: self.tb.to_guff(),
        }
    }
}

impl IfaceSettings {
    pub fn to_guff_iface(&self) -> guff_style::IfaceOptions {
        guff_style::IfaceOptions {
            enable: self.enable.clone(),
            unused_exclude: self.settings.unused.exclude.clone(),
        }
    }
}

impl BidichkSettings {
    pub fn to_guff_bidichk(&self) -> guff_style::BidichkOptions {
        let mut names = Vec::new();
        if self.left_to_right_embedding {
            names.push("LEFT-TO-RIGHT-EMBEDDING".into());
        }
        if self.right_to_left_embedding {
            names.push("RIGHT-TO-LEFT-EMBEDDING".into());
        }
        if self.pop_directional_formatting {
            names.push("POP-DIRECTIONAL-FORMATTING".into());
        }
        if self.left_to_right_override {
            names.push("LEFT-TO-RIGHT-OVERRIDE".into());
        }
        if self.right_to_left_override {
            names.push("RIGHT-TO-LEFT-OVERRIDE".into());
        }
        if self.left_to_right_isolate {
            names.push("LEFT-TO-RIGHT-ISOLATE".into());
        }
        if self.right_to_left_isolate {
            names.push("RIGHT-TO-LEFT-ISOLATE".into());
        }
        if self.first_strong_isolate {
            names.push("FIRST-STRONG-ISOLATE".into());
        }
        if self.pop_directional_isolate {
            names.push("POP-DIRECTIONAL-ISOLATE".into());
        }
        guff_style::BidichkOptions {
            disallowed_runes: names,
        }
    }
}

/// Parse golangci v1 `gomodguard` YAML into [`GomodguardSettings`].
///
/// Shape:
/// ```yaml
/// blocked:
///   modules:
///     - github.com/foo:
///         reason: "..."
///   local_replace_directives: true
/// ```
fn merge_gomodguard_v1(out: &mut GomodguardSettings, value: &serde_yaml::Value) {
    let Some(map) = value.as_mapping() else {
        return;
    };
    if let Some(blocked) = map
        .get(serde_yaml::Value::String("blocked".into()))
        .and_then(|v| v.as_mapping())
    {
        if let Some(modules) = blocked
            .get(serde_yaml::Value::String("modules".into()))
            .and_then(|v| v.as_sequence())
        {
            for entry in modules {
                if let Some(entry_map) = entry.as_mapping() {
                    for (k, v) in entry_map {
                        let Some(name) = k.as_str() else {
                            continue;
                        };
                        let reason = v
                            .as_mapping()
                            .and_then(|m| {
                                m.get(serde_yaml::Value::String("reason".into()))
                                    .and_then(|r| r.as_str())
                            })
                            .unwrap_or("")
                            .to_string();
                        out.blocked_modules.push((name.to_string(), reason));
                    }
                }
            }
        }
        if let Some(v) = blocked.get(serde_yaml::Value::String(
            "local_replace_directives".into(),
        )) {
            if let Some(b) = v.as_bool() {
                out.local_replace_directives = b;
            }
        }
    }
}

/// Parse golangci `gomodguard_v2` YAML into [`GomodguardSettings`].
///
/// Shape:
/// ```yaml
/// local-replace-directives: true
/// blocked:
///   - module: github.com/foo
///     reason: "..."
/// ```
fn merge_gomodguard_v2(out: &mut GomodguardSettings, value: &serde_yaml::Value) {
    let Some(map) = value.as_mapping() else {
        return;
    };
    if let Some(v) = map.get(serde_yaml::Value::String(
        "local-replace-directives".into(),
    )) {
        if let Some(b) = v.as_bool() {
            out.local_replace_directives = b;
        }
    }
    if let Some(blocked) = map
        .get(serde_yaml::Value::String("blocked".into()))
        .and_then(|v| v.as_sequence())
    {
        for entry in blocked {
            let Some(entry_map) = entry.as_mapping() else {
                continue;
            };
            let Some(module) = entry_map
                .get(serde_yaml::Value::String("module".into()))
                .and_then(|v| v.as_str())
            else {
                continue;
            };
            let reason = entry_map
                .get(serde_yaml::Value::String("reason".into()))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            out.blocked_modules
                .push((module.to_string(), reason));
        }
    }
}

fn convert_revive_argument(value: &serde_yaml::Value) -> guff_revive::RuleArgument {
    match value {
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                guff_revive::RuleArgument::Integer(i)
            } else {
                guff_revive::RuleArgument::String(n.to_string())
            }
        }
        serde_yaml::Value::String(s) => guff_revive::RuleArgument::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            guff_revive::RuleArgument::List(seq.iter().map(convert_revive_argument).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let mut out = std::collections::HashMap::new();
            for (k, v) in map {
                let key = match k {
                    serde_yaml::Value::String(s) => s.clone(),
                    serde_yaml::Value::Number(n) => n.to_string(),
                    other => format!("{other:?}"),
                };
                out.insert(key, convert_revive_argument(v));
            }
            guff_revive::RuleArgument::Map(out)
        }
        serde_yaml::Value::Bool(b) => guff_revive::RuleArgument::String(b.to_string()),
        serde_yaml::Value::Null => guff_revive::RuleArgument::String(String::new()),
        serde_yaml::Value::Tagged(tagged) => convert_revive_argument(&tagged.value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_errcheck_check_blank() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
errcheck:
  check-blank: true
  check-type-assertions: true
  disable-default-exclusions: true
  exclude-functions:
    - io.Copy
    - (net/http.ResponseWriter).Write
"#,
        )
        .unwrap();
        let s = LinterSettings::from_yaml(&yaml);
        assert!(s.errcheck.check_blank);
        assert!(s.errcheck.check_type_assertions);
        assert!(s.errcheck.disable_default_exclusions);
        assert_eq!(
            s.errcheck.exclude_functions,
            vec![
                "io.Copy".to_string(),
                "(net/http.ResponseWriter).Write".to_string(),
            ]
        );
    }

    #[test]
    fn staticcheck_checks_disable_one() {
        let settings = StaticcheckSettings {
            checks: Some(vec!["all".into(), "-SA1004".into()]),
            ..StaticcheckSettings::default()
        };
        let names = ["SA1004", "SA1000", "S1000"];
        let analyzers: Vec<&'static Analyzer> = names
            .iter()
            .map(|n| leak_name(n))
            .collect();
        let filtered = filter_staticcheck(&settings, analyzers);
        let kept: Vec<&str> = filtered.iter().map(|a| a.name).collect();
        assert!(!kept.contains(&"SA1004"));
        assert!(kept.contains(&"SA1000"));
        assert!(kept.contains(&"S1000"));
    }

    #[test]
    fn staticcheck_checks_disable_qf_glob() {
        // nats-server OSS hunt: checks: [all, -QF*, -ST1003, -ST1016]
        let settings = StaticcheckSettings {
            checks: Some(vec![
                "all".into(),
                "-QF*".into(),
                "-ST1003".into(),
                "-ST1016".into(),
            ]),
            ..StaticcheckSettings::default()
        };
        let names = ["QF1001", "QF1003", "S1000", "SA1000", "ST1003", "ST1005"];
        let analyzers: Vec<&'static Analyzer> = names.iter().map(|n| leak_name(n)).collect();
        let filtered = filter_staticcheck(&settings, analyzers);
        let kept: Vec<&str> = filtered.iter().map(|a| a.name).collect();
        assert!(!kept.contains(&"QF1001"));
        assert!(!kept.contains(&"QF1003"));
        assert!(!kept.contains(&"ST1003"));
        assert!(kept.contains(&"S1000"));
        assert!(kept.contains(&"SA1000"));
        assert!(kept.contains(&"ST1005"));
    }

    #[test]
    fn parse_revive_rules_settings() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
revive:
  severity: warning
  rules:
    - name: enforce-map-style
      arguments: ["make"]
    - name: comments-density
      arguments: [15]
      severity: error
"#,
        )
        .unwrap();
        let s = LinterSettings::from_yaml(&yaml);
        assert_eq!(s.revive.severity.as_deref(), Some("warning"));
        assert_eq!(s.revive.rules.as_ref().map(|r| r.len()), Some(2));
        assert_eq!(s.revive.rules.as_ref().unwrap()[0].name, "enforce-map-style");
        assert_eq!(
            s.revive.rules.as_ref().unwrap()[1].severity.as_deref(),
            Some("error")
        );
        let bag = s.to_bag();
        let revive = bag
            .get::<guff_revive::Settings>("revive")
            .expect("revive settings");
        assert_eq!(revive.severity.as_deref(), Some("warning"));
        assert!(revive.rule("enforce-map-style").is_some());
        assert_eq!(revive.rule_severity("comments-density"), Some("error"));
        assert_eq!(revive.rule_severity("enforce-map-style"), Some("warning"));
    }

    #[test]
    fn revive_settings_without_rules_match_golangci_empty_set() {
        // golangci-lint: any non-zero revive settings (e.g. confidence) without
        // enable-default-rules / rules list → empty rule set.
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
revive:
  confidence: 0.8
  severity: error
  enable-all-rules: false
"#,
        )
        .unwrap();
        let s = LinterSettings::from_yaml(&yaml);
        let bag = s.to_bag();
        let revive = bag
            .get::<guff_revive::Settings>("revive")
            .expect("revive settings");
        assert!(
            !revive.rule_enabled(
                "unused-parameter",
                guff_revive::DEFAULT_RULES,
                &[]
            ),
            "customized revive settings must not imply default rules"
        );
        assert!(
            !revive.rule_enabled("exported", guff_revive::DEFAULT_RULES, &[]),
            "customized revive settings must not imply default rules"
        );
    }

    #[test]
    fn revive_settings_absent_keep_default_rules() {
        let yaml: serde_yaml::Value = serde_yaml::from_str("{}",).unwrap();
        let s = LinterSettings::from_yaml(&yaml);
        let bag = s.to_bag();
        let revive = bag
            .get::<guff_revive::Settings>("revive")
            .expect("revive settings");
        assert!(revive.rule_enabled(
            "exported",
            guff_revive::DEFAULT_RULES,
            &[]
        ));
    }

    #[test]
    fn parse_custom_module_settings() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
custom:
  example:
    type: module
    description: find TODOs without an author
    settings:
      one: yes
"#,
        )
        .unwrap();
        let s = LinterSettings::from_yaml(&yaml);
        let cfg = s.custom.get("example").expect("example custom");
        assert_eq!(cfg.type_, "module");
        assert_eq!(cfg.description, "find TODOs without an author");
        let bag = s.to_bag();
        let nested = bag
            .get::<serde_yaml::Value>("example")
            .expect("example settings in bag");
        assert_eq!(
            nested
                .as_mapping()
                .unwrap()
                .get(serde_yaml::Value::String("one".into()))
                .and_then(|v| v.as_str()),
            Some("yes")
        );
    }

    #[test]
    fn govet_disable_all_plus_enable() {
        let settings = GovetSettings {
            disable_all: true,
            enable: vec!["printf".into()],
            ..GovetSettings::default()
        };
        let analyzers: Vec<&'static Analyzer> =
            ["printf", "assign", "shadow"].iter().map(|n| leak_name(n)).collect();
        let filtered = filter_govet(&settings, analyzers);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "printf");
    }

    fn leak_name(name: &'static str) -> &'static Analyzer {
        Box::leak(Box::new(Analyzer {
            name,
            doc: "",
            url: "",
            run: noop,
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        }))
    }

    fn noop(_: &mut guff_analysis::Pass<'_>) -> Result<Option<guff_analysis::AnalysisResult>, guff_analysis::RunError> {
        Ok(None)
    }
}
