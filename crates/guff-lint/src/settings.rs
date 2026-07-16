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
    pub godot: GodotSettings,
    pub godox: GodoxSettings,
    pub dupword: DupwordSettings,
    pub depguard: DepguardSettings,
    pub gomoddirectives: GomoddirectivesSettings,
    pub gomodguard: GomodguardSettings,
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
    // DEFERRED (R4 follow-up): disable-default-exclusions, exclude-functions, verbose.
}

/// `linters.settings.govet` / `linters-settings.govet`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GovetSettings {
    #[serde(default, rename = "enable-all")]
    pub enable_all: bool,
    #[serde(default, rename = "disable-all")]
    pub disable_all: bool,
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default)]
    pub disable: Vec<String>,
}

/// `linters.settings.staticcheck` / `linters-settings.staticcheck`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct StaticcheckSettings {
    /// Check selectors (`all`, `SA1000`, `-SA1000`, …). `None` = keep registry default.
    #[serde(default)]
    pub checks: Option<Vec<String>>,
    // DEFERRED: initialisms, dot-import-whitelist, http-status-code-whitelist.
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
    #[serde(default, rename = "ignore-words")]
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
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct FunlenSettings {
    #[serde(default)]
    pub lines: Option<usize>,
    #[serde(default)]
    pub statements: Option<usize>,
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
    #[serde(default)]
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
    #[serde(default, rename = "ignored-numbers")]
    pub ignored_numbers: Vec<String>,
    #[serde(default, rename = "ignored-files")]
    pub ignored_files: Vec<String>,
    #[serde(default, rename = "ignored-functions")]
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
    #[serde(default)]
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
    #[serde(default, rename = "allow-cuddle-with-calls")]
    pub allow_cuddle_with_calls: Vec<String>,
    #[serde(default, rename = "allow-cuddle-with-rhs")]
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
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default, rename = "allow-empty")]
    pub allow_empty: Option<bool>,
    #[serde(default, rename = "allow-empty-rx")]
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
    #[serde(default)]
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
    #[serde(default)]
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
    #[serde(default, rename = "allowed-keys")]
    pub allowed_keys: Vec<String>,
    #[serde(default, rename = "forbidden-keys")]
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
    #[serde(default = "default_true", rename = "require-string-msg")]
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

impl Default for TestifylintFormatterSettings {
    fn default() -> Self {
        Self {
            check_format_string: true,
            require_f_funcs: false,
            require_string_msg: true,
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
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default)]
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
    #[serde(default, rename = "extra-ignore-sigs")]
    pub extra_ignore_sigs: Vec<String>,
    #[serde(default, rename = "ignore-sig-regexps")]
    pub ignore_sig_regexps: Vec<String>,
    #[serde(default, rename = "ignore-package-globs")]
    pub ignore_package_globs: Vec<String>,
    #[serde(default, rename = "ignore-interface-regexps")]
    pub ignore_interface_regexps: Vec<String>,
    #[serde(default, rename = "report-internal-errors")]
    pub report_internal_errors: bool,
}

/// `linters.settings.godot` / `linters-settings.godot`.
///
/// `toplevel` / `noinline` scopes remain DEFERRED (fall back to declarations).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GodotSettings {
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
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
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// `linters.settings.dupword` / `linters-settings.dupword`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DupwordSettings {
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub ignore: Vec<String>,
    #[serde(default, rename = "comments-only")]
    pub comments_only: Option<bool>,
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
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
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
    #[serde(default, rename = "replace-allow-list")]
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

impl LinterSettings {
    /// Parse from v2 `linters.settings` or v1 `linters-settings` YAML mapping.
    pub fn from_yaml(value: &serde_yaml::Value) -> Self {
        let Some(map) = value.as_mapping() else {
            return Self::default();
        };
        let mut out = Self::default();
        if let Some(v) = map.get(serde_yaml::Value::String("errcheck".into())) {
            if let Ok(s) = serde_yaml::from_value::<ErrcheckSettings>(v.clone()) {
                out.errcheck = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("govet".into())) {
            if let Ok(s) = serde_yaml::from_value::<GovetSettings>(v.clone()) {
                out.govet = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("staticcheck".into())) {
            if let Ok(s) = serde_yaml::from_value::<StaticcheckSettings>(v.clone()) {
                out.staticcheck = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("revive".into())) {
            if let Ok(s) = serde_yaml::from_value::<ReviveSettings>(v.clone()) {
                out.revive = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("dupl".into())) {
            if let Ok(s) = serde_yaml::from_value::<DuplSettings>(v.clone()) {
                out.dupl = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("misspell".into())) {
            if let Ok(s) = serde_yaml::from_value::<MisspellSettings>(v.clone()) {
                out.misspell = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("gocyclo".into())) {
            if let Ok(s) = serde_yaml::from_value::<GocycloSettings>(v.clone()) {
                out.gocyclo = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("gocognit".into())) {
            if let Ok(s) = serde_yaml::from_value::<GocognitSettings>(v.clone()) {
                out.gocognit = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("nestif".into())) {
            if let Ok(s) = serde_yaml::from_value::<NestifSettings>(v.clone()) {
                out.nestif = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("dogsled".into())) {
            if let Ok(s) = serde_yaml::from_value::<DogsledSettings>(v.clone()) {
                out.dogsled = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("funlen".into())) {
            if let Ok(s) = serde_yaml::from_value::<FunlenSettings>(v.clone()) {
                out.funlen = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("cyclop".into())) {
            if let Ok(s) = serde_yaml::from_value::<CyclopSettings>(v.clone()) {
                out.cyclop = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("lll".into())) {
            if let Ok(s) = serde_yaml::from_value::<LllSettings>(v.clone()) {
                out.lll = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("nakedret".into())) {
            if let Ok(s) = serde_yaml::from_value::<NakedretSettings>(v.clone()) {
                out.nakedret = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("nlreturn".into())) {
            if let Ok(s) = serde_yaml::from_value::<NlreturnSettings>(v.clone()) {
                out.nlreturn = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("predeclared".into())) {
            if let Ok(s) = serde_yaml::from_value::<PredeclaredSettings>(v.clone()) {
                out.predeclared = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("whitespace".into())) {
            if let Ok(s) = serde_yaml::from_value::<WhitespaceSettings>(v.clone()) {
                out.whitespace = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("mnd".into())) {
            if let Ok(s) = serde_yaml::from_value::<MndSettings>(v.clone()) {
                out.mnd = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("prealloc".into())) {
            if let Ok(s) = serde_yaml::from_value::<PreallocSettings>(v.clone()) {
                out.prealloc = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("tagalign".into())) {
            if let Ok(s) = serde_yaml::from_value::<TagalignSettings>(v.clone()) {
                out.tagalign = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("wsl".into())) {
            if let Ok(s) = serde_yaml::from_value::<WslSettings>(v.clone()) {
                out.wsl = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("perfsprint".into())) {
            if let Ok(s) = serde_yaml::from_value::<PerfsprintSettings>(v.clone()) {
                out.perfsprint = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("goconst".into())) {
            if let Ok(s) = serde_yaml::from_value::<GoconstSettings>(v.clone()) {
                out.goconst = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("copyloopvar".into())) {
            if let Ok(s) = serde_yaml::from_value::<CopyloopvarSettings>(v.clone()) {
                out.copyloopvar = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("usetesting".into())) {
            if let Ok(s) = serde_yaml::from_value::<UsetestingSettings>(v.clone()) {
                out.usetesting = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("usestdlibvars".into())) {
            if let Ok(s) = serde_yaml::from_value::<UsestdlibvarsSettings>(v.clone()) {
                out.usestdlibvars = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("unconvert".into())) {
            if let Ok(s) = serde_yaml::from_value::<UnconvertSettings>(v.clone()) {
                out.unconvert = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("exhaustruct".into())) {
            if let Ok(s) = serde_yaml::from_value::<ExhaustructSettings>(v.clone()) {
                out.exhaustruct = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("exhaustive".into())) {
            if let Ok(s) = serde_yaml::from_value::<ExhaustiveSettings>(v.clone()) {
                out.exhaustive = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("musttag".into())) {
            if let Ok(s) = serde_yaml::from_value::<MusttagSettings>(v.clone()) {
                out.musttag = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("loggercheck".into())) {
            if let Ok(s) = serde_yaml::from_value::<LoggercheckSettings>(v.clone()) {
                out.loggercheck = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("sloglint".into())) {
            if let Ok(s) = serde_yaml::from_value::<SloglintSettings>(v.clone()) {
                out.sloglint = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("testifylint".into())) {
            if let Ok(s) = serde_yaml::from_value::<TestifylintSettings>(v.clone()) {
                out.testifylint = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("errchkjson".into())) {
            if let Ok(s) = serde_yaml::from_value::<ErrchkjsonSettings>(v.clone()) {
                out.errchkjson = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("wrapcheck".into())) {
            if let Ok(s) = serde_yaml::from_value::<WrapcheckSettings>(v.clone()) {
                out.wrapcheck = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("godot".into())) {
            if let Ok(s) = serde_yaml::from_value::<GodotSettings>(v.clone()) {
                out.godot = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("godox".into())) {
            if let Ok(s) = serde_yaml::from_value::<GodoxSettings>(v.clone()) {
                out.godox = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("dupword".into())) {
            if let Ok(s) = serde_yaml::from_value::<DupwordSettings>(v.clone()) {
                out.dupword = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("depguard".into())) {
            if let Ok(s) = serde_yaml::from_value::<DepguardSettings>(v.clone()) {
                out.depguard = s;
            }
        }
        if let Some(v) = map.get(serde_yaml::Value::String("gomoddirectives".into())) {
            if let Ok(s) = serde_yaml::from_value::<GomoddirectivesSettings>(v.clone()) {
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
            },
        );
        bag.insert("revive", self.revive.to_guff_revive());
        bag.insert("dupl", self.dupl.to_guff_dupl());
        bag.insert("misspell", self.misspell.to_guff_misspell());
        bag.insert("gocyclo", self.gocyclo.to_guff_gocyclo());
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
        bag.insert("godot", self.godot.to_guff_godot());
        bag.insert("godox", self.godox.to_guff_godox());
        bag.insert("dupword", self.dupword.to_guff_dupword());
        bag.insert("depguard", self.depguard.to_guff_depguard());
        bag.insert(
            "gomoddirectives",
            self.gomoddirectives.to_guff_gomoddirectives(),
        );
        bag.insert("gomodguard", self.gomodguard.to_guff_gomodguard());
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
    let Some(checks) = settings.checks.as_ref() else {
        return analyzers;
    };
    if checks.is_empty() {
        return analyzers;
    }

    let mut allow_all = false;
    let mut enabled: HashSet<String> = HashSet::new();
    let mut disabled: HashSet<String> = HashSet::new();

    for c in checks {
        if c == "all" {
            allow_all = true;
            continue;
        }
        if let Some(rest) = c.strip_prefix('-') {
            disabled.insert(rest.to_string());
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

impl ReviveSettings {
    pub fn to_guff_revive(&self) -> guff_revive::Settings {
        let rules = self.rules.as_ref().map(|rules| {
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
        guff_revive::Settings {
            severity: self.severity.clone(),
            rules,
            confidence: self.confidence,
            ignore_generated_header: self.ignore_generated_header,
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
            lines: self.lines.unwrap_or(defaults.lines),
            statements: self.statements.unwrap_or(defaults.statements),
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
"#,
        )
        .unwrap();
        let s = LinterSettings::from_yaml(&yaml);
        assert!(s.errcheck.check_blank);
        assert!(s.errcheck.check_type_assertions);
    }

    #[test]
    fn staticcheck_checks_disable_one() {
        let settings = StaticcheckSettings {
            checks: Some(vec!["all".into(), "-SA1004".into()]),
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
