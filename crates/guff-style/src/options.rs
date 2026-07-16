//! Per-linter options from `linters.settings` (wired by `guff-lint`).

/// `linters.settings.gocyclo` / `linters-settings.gocyclo`.
#[derive(Debug, Clone, Copy)]
pub struct GocycloOptions {
    pub min_complexity: usize,
}

impl Default for GocycloOptions {
    fn default() -> Self {
        Self {
            min_complexity: 30,
        }
    }
}

/// `linters.settings.gocognit` / `linters-settings.gocognit`.
#[derive(Debug, Clone, Copy)]
pub struct GocognitOptions {
    pub min_complexity: usize,
}

impl Default for GocognitOptions {
    fn default() -> Self {
        Self {
            min_complexity: 30,
        }
    }
}

/// `linters.settings.nestif` / `linters-settings.nestif`.
#[derive(Debug, Clone, Copy)]
pub struct NestifOptions {
    pub min_complexity: usize,
}

impl Default for NestifOptions {
    fn default() -> Self {
        Self {
            min_complexity: 5,
        }
    }
}

/// `linters.settings.dogsled` / `linters-settings.dogsled`.
#[derive(Debug, Clone, Copy)]
pub struct DogsledOptions {
    pub max_blank_identifiers: usize,
}

impl Default for DogsledOptions {
    fn default() -> Self {
        Self {
            max_blank_identifiers: 2,
        }
    }
}

/// `linters.settings.funlen` / `linters-settings.funlen`.
#[derive(Debug, Clone, Copy)]
pub struct FunlenOptions {
    pub lines: usize,
    pub statements: usize,
    pub ignore_comments: bool,
}

impl Default for FunlenOptions {
    fn default() -> Self {
        Self {
            lines: 60,
            statements: 40,
            ignore_comments: true,
        }
    }
}

/// `linters.settings.cyclop` / `linters-settings.cyclop`.
#[derive(Debug, Clone, Copy)]
pub struct CyclopOptions {
    pub max_complexity: usize,
    /// When > 0, report if package-average cyclomatic complexity exceeds this.
    pub package_average: f64,
    /// Skip `Test*` functions (golangci `skip-tests`).
    pub skip_tests: bool,
}

impl Default for CyclopOptions {
    fn default() -> Self {
        Self {
            max_complexity: 10,
            package_average: 0.0,
            skip_tests: false,
        }
    }
}

/// `linters.settings.lll` / `linters-settings.lll`.
#[derive(Debug, Clone, Copy)]
pub struct LllOptions {
    pub line_length: usize,
    pub tab_width: usize,
}

impl Default for LllOptions {
    fn default() -> Self {
        Self {
            line_length: 120,
            tab_width: 1,
        }
    }
}

/// `linters.settings.nakedret` / `linters-settings.nakedret`.
#[derive(Debug, Clone, Copy)]
pub struct NakedretOptions {
    pub max_func_lines: usize,
    pub skip_test_files: bool,
}

impl Default for NakedretOptions {
    fn default() -> Self {
        Self {
            max_func_lines: 30,
            skip_test_files: false,
        }
    }
}

/// `linters.settings.predeclared` / `linters-settings.predeclared`.
#[derive(Debug, Clone)]
pub struct PredeclaredOptions {
    pub ignore: Vec<String>,
    pub qualified: bool,
}

impl Default for PredeclaredOptions {
    fn default() -> Self {
        Self {
            ignore: Vec::new(),
            qualified: false,
        }
    }
}

/// `linters.settings.whitespace` / `linters-settings.whitespace`.
#[derive(Debug, Clone, Copy)]
pub struct WhitespaceOptions {
    pub multi_if: bool,
    pub multi_func: bool,
}

impl Default for WhitespaceOptions {
    fn default() -> Self {
        Self {
            multi_if: false,
            multi_func: false,
        }
    }
}

/// `linters.settings.mnd` / `linters-settings.mnd`.
#[derive(Debug, Clone)]
pub struct MndOptions {
    pub checks: Vec<String>,
    pub ignored_numbers: Vec<String>,
    pub ignored_files: Vec<String>,
    pub ignored_functions: Vec<String>,
}

impl Default for MndOptions {
    fn default() -> Self {
        Self {
            checks: vec![
                "argument".into(),
                "case".into(),
                "condition".into(),
                "operation".into(),
                "return".into(),
                "assign".into(),
            ],
            ignored_numbers: Vec::new(),
            ignored_files: Vec::new(),
            ignored_functions: Vec::new(),
        }
    }
}

impl MndOptions {
    pub fn check_enabled(&self, name: &str) -> bool {
        self.checks.iter().any(|c| c == name)
    }
}

/// `linters.settings.prealloc` / `linters-settings.prealloc`.
#[derive(Debug, Clone, Copy)]
pub struct PreallocOptions {
    pub simple: bool,
    pub range_loops: bool,
    pub for_loops: bool,
}

impl Default for PreallocOptions {
    fn default() -> Self {
        Self {
            simple: true,
            range_loops: true,
            for_loops: false,
        }
    }
}

/// `linters.settings.tagalign` / `linters-settings.tagalign`.
#[derive(Debug, Clone)]
pub struct TagalignOptions {
    pub align: bool,
    pub sort: bool,
    pub order: Vec<String>,
    pub strict: bool,
}

impl Default for TagalignOptions {
    fn default() -> Self {
        Self {
            align: true,
            sort: true,
            order: Vec::new(),
            strict: false,
        }
    }
}

/// `linters.settings.wsl` / `linters-settings.wsl`.
#[derive(Debug, Clone)]
pub struct WslOptions {
    pub strict_append: bool,
    pub allow_assign_and_call: bool,
    pub allow_assign_and_anything: bool,
    pub allow_multiline_assign: bool,
    pub allow_cuddle_with_calls: Vec<String>,
    pub allow_cuddle_with_rhs: Vec<String>,
}

impl Default for WslOptions {
    fn default() -> Self {
        Self {
            strict_append: true,
            allow_assign_and_call: true,
            allow_assign_and_anything: false,
            allow_multiline_assign: true,
            allow_cuddle_with_calls: vec!["Lock".into(), "RLock".into()],
            allow_cuddle_with_rhs: vec!["Unlock".into(), "RUnlock".into()],
        }
    }
}

/// `linters.settings.perfsprint` / `linters-settings.perfsprint`.
#[derive(Debug, Clone, Copy)]
pub struct PerfsprintOptions {
    pub integer_format: bool,
    pub int_conversion: bool,
    pub error_format: bool,
    pub err_error: bool,
    pub errorf: bool,
    pub string_format: bool,
    pub sprintf1: bool,
    pub strconcat: bool,
    pub bool_format: bool,
    pub hex_format: bool,
    /// golangci / upstream `concat-loop` (default true).
    pub concat_loop: bool,
    /// golangci / upstream `loop-other-ops` (default false).
    pub loop_other_ops: bool,
}

impl Default for PerfsprintOptions {
    fn default() -> Self {
        Self {
            integer_format: true,
            int_conversion: true,
            error_format: true,
            err_error: false,
            errorf: true,
            string_format: true,
            sprintf1: true,
            strconcat: true,
            bool_format: true,
            hex_format: true,
            concat_loop: true,
            loop_other_ops: false,
        }
    }
}

/// `linters.settings.goconst` / `linters-settings.goconst`.
#[derive(Debug, Clone, Copy)]
pub struct GoconstOptions {
    pub min_len: usize,
    pub min_occurrences: usize,
    /// golangci `ignore-calls`: when true, skip string literals in call arguments.
    pub ignore_calls: bool,
    pub ignore_tests: bool,
    /// golangci `match-constant`: match repeated literals against existing `const` values.
    pub match_constant: bool,
    /// golangci `find-duplicates`: report constants that share the same value.
    pub find_duplicates: bool,
    /// golangci `numbers`: also report duplicated numeric literals.
    pub numbers: bool,
    /// golangci `min` (only when `numbers` is true).
    pub number_min: i64,
    /// golangci `max` (only when `numbers` is true).
    pub number_max: i64,
}

impl Default for GoconstOptions {
    fn default() -> Self {
        Self {
            min_len: 3,
            min_occurrences: 3,
            ignore_calls: true,
            ignore_tests: false,
            match_constant: true,
            find_duplicates: false,
            numbers: false,
            number_min: 3,
            number_max: 3,
        }
    }
}

/// `linters.settings.nlreturn` / `linters-settings.nlreturn`.
#[derive(Debug, Clone, Copy)]
pub struct NlreturnOptions {
    pub block_size: i64,
}

impl Default for NlreturnOptions {
    fn default() -> Self {
        Self {
            block_size: 1,
        }
    }
}

/// `linters.settings.copyloopvar` / `linters-settings.copyloopvar`.
#[derive(Debug, Clone, Copy)]
pub struct CopyloopvarOptions {
    /// golangci `check-alias`: also report ` _i := i` alias copies (default false).
    pub check_alias: bool,
}

impl Default for CopyloopvarOptions {
    fn default() -> Self {
        Self { check_alias: false }
    }
}

/// `linters.settings.usetesting` / `linters-settings.usetesting`.
///
/// Defaults match upstream ldez/usetesting (not golangci's overridden defaults).
#[derive(Debug, Clone, Copy)]
pub struct UsetestingOptions {
    pub os_create_temp: bool,
    pub os_mkdir_temp: bool,
    pub os_setenv: bool,
    pub os_temp_dir: bool,
    pub os_chdir: bool,
    pub context_background: bool,
    pub context_todo: bool,
}

impl Default for UsetestingOptions {
    fn default() -> Self {
        Self {
            os_create_temp: true,
            os_mkdir_temp: true,
            os_setenv: false,
            os_temp_dir: false,
            os_chdir: true,
            context_background: false,
            context_todo: false,
        }
    }
}

/// `linters.settings.usestdlibvars` / `linters-settings.usestdlibvars`.
#[derive(Debug, Clone, Copy)]
pub struct UsestdlibvarsOptions {
    pub http_method: bool,
    pub http_status_code: bool,
    /// Optional (golangci / upstream default: false).
    pub time_weekday: bool,
    pub time_month: bool,
    pub time_layout: bool,
    pub crypto_hash: bool,
    /// YAML key: `default-rpc-path` (upstream flag: `rpc-default-path`).
    pub default_rpc_path: bool,
    pub sql_isolation_level: bool,
    pub tls_signature_scheme: bool,
    pub constant_kind: bool,
    pub time_date_month: bool,
}

impl Default for UsestdlibvarsOptions {
    fn default() -> Self {
        Self {
            http_method: true,
            http_status_code: true,
            time_weekday: false,
            time_month: false,
            time_layout: false,
            crypto_hash: false,
            default_rpc_path: false,
            sql_isolation_level: false,
            tls_signature_scheme: false,
            constant_kind: false,
            time_date_month: false,
        }
    }
}

/// `linters.settings.unconvert` / `linters-settings.unconvert`.
#[derive(Debug, Clone, Copy)]
pub struct UnconvertOptions {
    /// Report float/complex identity conversions (default off; Go 1.9+ rounding).
    pub fast_math: bool,
    /// More conservative reporting (parent-context filtering).
    ///
    /// DEFERRED: full `isSafeContext` — currently ignored.
    pub safe: bool,
}

impl Default for UnconvertOptions {
    fn default() -> Self {
        Self {
            fast_math: false,
            safe: false,
        }
    }
}

/// `linters.settings.exhaustive` / `linters-settings.exhaustive`.
#[derive(Debug, Clone)]
pub struct ExhaustiveOptions {
    /// Check switch statements (default true).
    pub check_switch: bool,
    /// Check map literals keyed by enum types.
    ///
    /// DEFERRED: map checking not implemented yet.
    pub check_map: bool,
    /// A `default` case makes the switch exhaustive without listing all members.
    pub default_signifies_exhaustive: bool,
    /// Require a `default` case even when all members are listed.
    pub default_case_required: bool,
    /// Regex of `pkg.Member` names to ignore.
    pub ignore_enum_members: String,
    /// Regex of `pkg.Type` names to ignore.
    pub ignore_enum_types: String,
    /// Only consider package-scope enums (not nested function scopes).
    pub package_scope_only: bool,
}

impl Default for ExhaustiveOptions {
    fn default() -> Self {
        Self {
            check_switch: true,
            check_map: false,
            default_signifies_exhaustive: false,
            default_case_required: false,
            ignore_enum_members: String::new(),
            ignore_enum_types: String::new(),
            package_scope_only: false,
        }
    }
}

/// One custom function entry for `linters.settings.musttag.functions`.
#[derive(Debug, Clone)]
pub struct MusttagFunc {
    /// Full function name, e.g. `encoding/json.Marshal` or `(*pkg.T).Method`.
    pub name: String,
    /// Struct tag key to require (`json`, `yaml`, …).
    pub tag: String,
    /// 0-based argument position to check.
    pub arg_pos: usize,
}

/// `linters.settings.musttag` / `linters-settings.musttag`.
#[derive(Debug, Clone, Default)]
pub struct MusttagOptions {
    /// Extra functions beyond upstream builtins.
    pub functions: Vec<MusttagFunc>,
}

/// `linters.settings.loggercheck` / `linters-settings.loggercheck`.
///
/// Checker bools match golangci-lint (`true` = enabled). Defaults enable all
/// five libraries (unlike standalone loggercheck which disables kitlog).
#[derive(Debug, Clone)]
pub struct LoggercheckOptions {
    pub kitlog: bool,
    pub klog: bool,
    pub logr: bool,
    pub slog: bool,
    pub zap: bool,
    pub require_string_key: bool,
    pub no_printf_like: bool,
    /// Extra fully-qualified function rules (upstream `-rules` / YAML `rules`).
    pub rules: Vec<String>,
}

impl Default for LoggercheckOptions {
    fn default() -> Self {
        Self {
            kitlog: true,
            klog: true,
            logr: true,
            slog: true,
            zap: true,
            require_string_key: false,
            no_printf_like: false,
            rules: Vec::new(),
        }
    }
}

/// Custom slog-like function entry for `linters.settings.sloglint.custom-funcs`.
#[derive(Debug, Clone)]
pub struct SloglintFunc {
    pub name: String,
    pub msg_pos: i32,
    pub args_pos: i32,
}

/// `linters.settings.testifylint` / `linters-settings.testifylint`.
///
/// Checker selection matches golangci-lint (`enable-all` / `disable-all` /
/// `enable` / `disable`). Only implemented checkers are honored; unknown
/// names are ignored.
#[derive(Debug, Clone)]
pub struct TestifylintOptions {
    pub enable_all: bool,
    pub disable_all: bool,
    pub enable: Vec<String>,
    pub disable: Vec<String>,
    /// `bool-compare.ignore-custom-types` (default false).
    pub bool_compare_ignore_custom_types: bool,
    /// `expected-actual.pattern`. `None` → upstream default pattern.
    pub expected_actual_pattern: Option<String>,
}

impl Default for TestifylintOptions {
    fn default() -> Self {
        Self {
            enable_all: false,
            disable_all: false,
            enable: Vec::new(),
            disable: Vec::new(),
            bool_compare_ignore_custom_types: false,
            expected_actual_pattern: None,
        }
    }
}

/// `linters.settings.sloglint` / `linters-settings.sloglint`.
///
/// Defaults match golangci-lint (`no-mixed-args: true`; other checks off).
#[derive(Debug, Clone)]
pub struct SloglintOptions {
    pub no_mixed_args: bool,
    pub kv_only: bool,
    pub attr_only: bool,
    /// `"all"` or `"default"`; empty = disabled.
    pub no_global: Option<String>,
    /// `"all"` or `"scope"`; empty = disabled.
    pub context: Option<String>,
    pub static_msg: bool,
    /// `"lowercased"` or `"capitalized"`; empty = disabled.
    pub msg_style: Option<String>,
    pub no_raw_keys: bool,
    /// `"snake"`, `"kebab"`, `"camel"`, or `"pascal"`; empty = disabled.
    pub key_naming_case: Option<String>,
    pub allowed_keys: Vec<String>,
    pub forbidden_keys: Vec<String>,
    pub args_on_sep_lines: bool,
    pub custom_funcs: Vec<SloglintFunc>,
}

impl Default for SloglintOptions {
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

/// `linters.settings.exhaustruct` / `linters-settings.exhaustruct`.
#[derive(Debug, Clone)]
pub struct ExhaustructOptions {
    /// Regexes matching full type names (`path.Name`) that should be checked.
    /// Empty = check all (subject to `exclude`).
    pub include: Vec<String>,
    /// Regexes matching types to skip (precedence over `include`).
    pub exclude: Vec<String>,
    /// Allow empty struct literals globally.
    pub allow_empty: bool,
    /// Regexes for types allowed to be empty.
    pub allow_empty_rx: Vec<String>,
    /// Allow empty structs in return statements.
    pub allow_empty_returns: bool,
    /// Allow empty structs in `var` / `:=` declarations.
    pub allow_empty_declarations: bool,
}

impl Default for ExhaustructOptions {
    fn default() -> Self {
        Self {
            include: Vec::new(),
            exclude: Vec::new(),
            allow_empty: false,
            allow_empty_rx: Vec::new(),
            allow_empty_returns: false,
            allow_empty_declarations: false,
        }
    }
}

impl UsestdlibvarsOptions {
    /// True when any check that walks arbitrary string/int literals is enabled.
    pub fn any_literal_table(&self) -> bool {
        self.time_weekday
            || self.time_month
            || self.time_layout
            || self.crypto_hash
            || self.default_rpc_path
            || self.sql_isolation_level
            || self.tls_signature_scheme
            || self.constant_kind
    }

    /// True when any check is enabled.
    pub fn any_enabled(&self) -> bool {
        self.http_method
            || self.http_status_code
            || self.any_literal_table()
            || self.time_date_month
    }
}
