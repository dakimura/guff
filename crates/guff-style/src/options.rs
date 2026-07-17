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

/// `linters.settings.maintidx` / `linters-settings.maintidx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaintidxOptions {
    /// Report functions with maintainability index strictly below this value.
    /// Upstream / golangci default: 20.
    pub under: usize,
}

impl Default for MaintidxOptions {
    fn default() -> Self {
        Self { under: 20 }
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

/// `suite-extra-assert-call.mode` (golangci / upstream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SuiteExtraAssertCallMode {
    /// Flag `s.Assert().Equal` → prefer `s.Equal` (upstream default).
    #[default]
    Remove,
    /// Flag `s.Equal` → prefer `s.Assert().Equal`.
    Require,
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
    /// `time-compare.suppress-calls-pattern`. `None` → upstream default.
    pub time_compare_suppress_calls_pattern: Option<String>,
    /// `formatter.check-format-string` (default true). Full printf parity DEFERRED.
    pub formatter_check_format_string: bool,
    /// `formatter.require-f-funcs` (default false).
    pub formatter_require_f_funcs: bool,
    /// `formatter.require-string-msg` (default true).
    pub formatter_require_string_msg: bool,
    /// `suite-extra-assert-call.mode` (default `remove`).
    pub suite_extra_assert_call_mode: SuiteExtraAssertCallMode,
    /// `require-error.fn-pattern`. `None` → all error assertion names.
    pub require_error_fn_pattern: Option<String>,
    /// `go-require.ignore-http-handlers` (default false).
    pub go_require_ignore_http_handlers: bool,
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
            time_compare_suppress_calls_pattern: None,
            formatter_check_format_string: true,
            formatter_require_f_funcs: false,
            formatter_require_string_msg: true,
            suite_extra_assert_call_mode: SuiteExtraAssertCallMode::Remove,
            require_error_fn_pattern: None,
            go_require_ignore_http_handlers: false,
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

/// `linters.settings.modernize` / `linters-settings.modernize`.
///
/// By default all implemented checkers are enabled. Names in `disable` are
/// skipped (golangci-lint compatible). Unknown / deferred checker names are
/// accepted so configs that disable `atomictypes` / `embedlit` etc. still parse.
#[derive(Debug, Clone, Default)]
pub struct ModernizeOptions {
    pub disable: Vec<String>,
}

/// `linters.settings.gocritic` / `linters-settings.gocritic`.
///
/// Selection mirrors golangci-lint:
/// - default → implemented stable checks
/// - `enable-all` → all implemented checks
/// - `disable-all` → none (then `enabled-checks`)
/// - `enabled-checks` / `disabled-checks` add/remove names
///
/// Unknown / deferred check names are accepted so prometheus-style configs
/// that disable unimplemented checks still parse.
#[derive(Debug, Clone, Default)]
pub struct GocriticOptions {
    pub enable_all: bool,
    pub disable_all: bool,
    pub enabled_checks: Vec<String>,
    pub disabled_checks: Vec<String>,
}

/// One forbidigo pattern (`linters.settings.forbidigo.forbid` entry).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForbidigoPattern {
    pub pattern: String,
    pub pkg: String,
    pub msg: String,
}

/// `linters.settings.asasalint` / `linters-settings.asasalint`.
///
/// `use_builtin_exclusions` defaults to true (golangci-lint). Extra `exclude`
/// regexes are merged with the builtin print/log pattern set.
#[derive(Debug, Clone)]
pub struct AsasalintOptions {
    pub exclude: Vec<String>,
    pub use_builtin_exclusions: bool,
}

impl Default for AsasalintOptions {
    fn default() -> Self {
        Self {
            exclude: Vec::new(),
            use_builtin_exclusions: true,
        }
    }
}

/// `linters.settings.iface` / `linters-settings.iface`.
///
/// Empty `enable` → golangci default (`identical` only).
/// `unused_exclude` maps to `settings.unused.exclude` (exact package paths).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IfaceOptions {
    pub enable: Vec<String>,
    pub unused_exclude: Vec<String>,
}

/// `linters.settings.reassign` / `linters-settings.reassign`.
///
/// Empty `patterns` → upstream default `^(Err.*|EOF)$`.
/// Non-empty → golangci joins as `^(p1|p2|…)$`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReassignOptions {
    pub patterns: Vec<String>,
}

/// `linters.settings.recvcheck` / `linters-settings.recvcheck`.
///
/// `disable_builtin` false (default) keeps Unmarshal*/GobDecode excludes.
/// `exclusions` format: `Struct.Method` or `*.Method`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecvcheckOptions {
    pub disable_builtin: bool,
    pub exclusions: Vec<String>,
}

/// `linters.settings.decorder` / `linters-settings.decorder`.
///
/// Golangci-lint defaults disable all check families (`disable-*-check: true`).
/// Use [`DecorderOptions::enabled`] for upstream analyzer defaults (all on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecorderOptions {
    /// Required order of declaration kinds (golangci `dec-order`).
    pub dec_order: Vec<String>,
    /// Ignore `_` vars for order/num checks.
    pub ignore_underscore_vars: bool,
    /// Disable all declaration-count checks.
    pub disable_dec_num_check: bool,
    /// Disable type declaration-count check only.
    pub disable_type_dec_num_check: bool,
    /// Disable const declaration-count check only.
    pub disable_const_dec_num_check: bool,
    /// Disable var declaration-count check only.
    pub disable_var_dec_num_check: bool,
    /// Disable declaration-order check.
    pub disable_dec_order_check: bool,
    /// Disable “init must be first function” check.
    pub disable_init_func_first_check: bool,
}

impl Default for DecorderOptions {
    fn default() -> Self {
        // Match golangci-lint `DecorderSettings` defaults (all checks off).
        Self {
            dec_order: vec![
                "type".into(),
                "const".into(),
                "var".into(),
                "func".into(),
            ],
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

impl DecorderOptions {
    /// Upstream analyzer flag defaults (all checks enabled).
    pub fn enabled() -> Self {
        Self {
            disable_dec_num_check: false,
            disable_dec_order_check: false,
            disable_init_func_first_check: false,
            ..Self::default()
        }
    }
}

/// `linters.settings.iotamixing` / `linters-settings.iotamixing`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IotamixingOptions {
    /// Report each valued const instead of the whole `const` block
    /// (golangci / upstream `report-individual`, default false).
    pub report_individual: bool,
}

/// `linters.settings.grouper` / `linters-settings.grouper`.
///
/// All flags default to **false** (golangci / upstream). Use [`GrouperOptions::enabled`]
/// to turn every check on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GrouperOptions {
    /// Require a single global `const` declaration only.
    pub const_require_single_const: bool,
    /// Require grouped global `const` declarations.
    pub const_require_grouping: bool,
    /// Require a single `import` declaration only.
    pub import_require_single_import: bool,
    /// Require grouped `import` declarations.
    pub import_require_grouping: bool,
    /// Require a single global `type` declaration only.
    pub type_require_single_type: bool,
    /// Require grouped global `type` declarations.
    pub type_require_grouping: bool,
    /// Require a single global `var` declaration only.
    pub var_require_single_var: bool,
    /// Require grouped global `var` declarations.
    pub var_require_grouping: bool,
}

impl GrouperOptions {
    /// All checks enabled (useful for tests / explicit configs).
    pub fn enabled() -> Self {
        Self {
            const_require_single_const: true,
            const_require_grouping: true,
            import_require_single_import: true,
            import_require_grouping: true,
            type_require_single_type: true,
            type_require_grouping: true,
            var_require_single_var: true,
            var_require_grouping: true,
        }
    }
}

/// `linters.settings.interfacebloat` / `linters-settings.interfacebloat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterfacebloatOptions {
    /// Maximum number of methods allowed inside an interface (upstream `max`).
    pub max: usize,
}

impl Default for InterfacebloatOptions {
    fn default() -> Self {
        Self { max: 10 }
    }
}

/// `linters.settings.embeddedstructfieldcheck` /
/// `linters-settings.embeddedstructfieldcheck`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddedstructfieldcheckOptions {
    /// Require a blank line between embedded and regular fields.
    /// Upstream / golangci default: true.
    pub empty_line: bool,
    /// Forbid embedding `sync.Mutex` / `sync.RWMutex`.
    /// Upstream / golangci default: false.
    pub forbid_mutex: bool,
}

impl Default for EmbeddedstructfieldcheckOptions {
    fn default() -> Self {
        Self {
            empty_line: true,
            forbid_mutex: false,
        }
    }
}

/// `linters.settings.gochecksumtype` / `linters-settings.gochecksumtype`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GochecksumtypeOptions {
    /// A non-panicking `default` case satisfies exhaustiveness.
    /// Upstream / golangci default: true.
    pub default_signifies_exhaustive: bool,
    /// Treat covering shared interfaces as exhaustive (skip listing all structs).
    /// Upstream / golangci default: false.
    pub include_shared_interfaces: bool,
}

impl Default for GochecksumtypeOptions {
    fn default() -> Self {
        Self {
            default_signifies_exhaustive: true,
            include_shared_interfaces: false,
        }
    }
}

/// `linters.settings.inamedparam` / `linters-settings.inamedparam`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InamedparamOptions {
    /// Skip interface methods that have exactly one parameter field.
    /// Upstream flag / golangci key: `skip-single-param` (default false).
    pub skip_single_param: bool,
}

/// `linters.settings.ireturn` / `linters-settings.ireturn`.
///
/// Default (both empty): allow `anon` / `error` / `empty` / `stdlib`.
/// `allow` and `reject` are mutually exclusive upstream; if both are set we
/// prefer `reject` (DEFERRED: hard error).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IreturnOptions {
    /// Allow-list of keywords / regexes (golangci `allow`).
    pub allow: Vec<String>,
    /// Reject-list of keywords / regexes (golangci `reject`).
    pub reject: Vec<String>,
}

/// `linters.settings.gosec` / `linters-settings.gosec`.
///
/// `includes` / `excludes` filter by rule id (`G101`, `G501`, …). Empty
/// `includes` means all implemented rules; `excludes` removes from that set.
/// `severity` / `confidence` / `config` / concurrency are DEFERRED.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GosecOptions {
    /// Only run these rule ids (golangci `includes`).
    pub includes: Vec<String>,
    /// Skip these rule ids (golangci `excludes`).
    pub excludes: Vec<String>,
}

/// `linters.settings.nonamedreturns` / `linters-settings.nonamedreturns`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NonamedreturnsOptions {
    /// Report named `error` returns even when used in defer (upstream default false).
    pub report_error_in_defer: bool,
    /// Allow unused named returns; report only if referenced or used by naked return.
    pub allow_unused_named_returns: bool,
}

/// `linters.settings.funcorder` / `linters-settings.funcorder`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FuncorderOptions {
    /// Check that constructors are placed after the struct declaration and
    /// before the struct's methods (upstream default true).
    pub constructor: bool,
    /// Check that exported methods are placed before unexported methods
    /// (upstream default true).
    pub struct_method: bool,
    /// Check that constructors / methods are sorted alphabetically within
    /// their group (upstream default false).
    pub alphabetical: bool,
    /// Check that exported top-level functions are placed before unexported
    /// ones (upstream default false).
    pub function: bool,
}

impl Default for FuncorderOptions {
    fn default() -> Self {
        Self {
            constructor: true,
            struct_method: true,
            alphabetical: false,
            function: false,
        }
    }
}

/// `linters.settings.paralleltest` / `linters-settings.paralleltest`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParalleltestOptions {
    /// Ignore missing top-level `t.Parallel()` calls (upstream `-i`).
    pub ignore_missing: bool,
    /// Ignore missing `t.Parallel()` in subtests / range runs.
    pub ignore_missing_subtests: bool,
    /// Report `defer` used together with `t.Parallel` (prefer `t.Cleanup`).
    pub check_cleanup: bool,
}

/// `linters.settings.tagliatelle` / `linters-settings.tagliatelle`.
///
/// Golangci-lint merges user `case.rules` onto defaults
/// (`json`/`yaml` → `camel`, `header` → `header`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagliatelleOptions {
    /// Tag-key → case-converter name (`camel`, `snake`, `header`, …).
    pub rules: std::collections::HashMap<String, String>,
    /// Extended rule key → case name (ExtraInitialisms DEFERRED; treated as simple case).
    pub extended_rules: std::collections::HashMap<String, String>,
    /// Compare converter(fieldName) instead of converter(tagValue).
    pub use_field_name: bool,
    /// Field names to skip.
    pub ignored_fields: Vec<String>,
    /// Skip the whole package (used by package overrides; DEFERRED radix).
    pub ignore: bool,
}

/// `linters.settings.testpackage` / `linters-settings.testpackage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestpackageOptions {
    /// Regexp matched against the test file path; matches are skipped.
    /// Upstream / golangci default: `(export|internal)_test\.go`.
    pub skip_regexp: String,
    /// Package names that may appear in `*_test.go` without a `_test` suffix.
    /// Upstream / golangci default: `["main"]`.
    pub allow_packages: Vec<String>,
}

impl Default for TestpackageOptions {
    fn default() -> Self {
        Self {
            skip_regexp: r"(export|internal)_test\.go".into(),
            allow_packages: vec!["main".into()],
        }
    }
}

/// Per-kind options for `linters.settings.thelper.{test,fuzz,benchmark,tb}`.
///
/// Defaults match kulti/thelper / golangci-lint (all checks on).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThelperKindOptions {
    pub first: bool,
    pub name: bool,
    pub begin: bool,
}

impl Default for ThelperKindOptions {
    fn default() -> Self {
        Self {
            first: true,
            name: true,
            begin: true,
        }
    }
}

/// `linters.settings.thelper` / `linters-settings.thelper`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ThelperOptions {
    pub test: ThelperKindOptions,
    pub fuzz: ThelperKindOptions,
    pub benchmark: ThelperKindOptions,
    pub tb: ThelperKindOptions,
}

/// `linters.settings.gosmopolitan` / `linters-settings.gosmopolitan`.
///
/// `watch_for_scripts` defaults to `["Han"]` (golangci-lint). Empty → `Han`.
/// `allow_time_local` defaults to false (i.e. `time.Local` is reported).
#[derive(Debug, Clone)]
pub struct GosmopolitanOptions {
    pub allow_time_local: bool,
    pub escape_hatches: Vec<String>,
    pub watch_for_scripts: Vec<String>,
}

impl Default for GosmopolitanOptions {
    fn default() -> Self {
        Self {
            allow_time_local: false,
            escape_hatches: Vec::new(),
            watch_for_scripts: vec!["Han".to_string()],
        }
    }
}

/// `linters.settings.goheader` / `linters-settings.goheader`.
///
/// Empty `template` and `template_path` → analyzer is a no-op.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoheaderOptions {
    pub template: String,
    pub template_path: String,
    pub const_values: std::collections::HashMap<String, String>,
    pub regexp_values: std::collections::HashMap<String, String>,
}

/// `linters.settings.forbidigo` / `linters-settings.forbidigo`.
///
/// `linters.settings.bidichk` — full rune names to check.
/// Empty list → all nine default dangerous runes (golangci-lint compat).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BidichkOptions {
    pub disallowed_runes: Vec<String>,
}

/// Empty `forbid` → upstream default `^(fmt\.Print(|f|ln)|print|println)$`.
/// `exclude_godoc_examples` defaults to true (golangci-lint).
/// `analyze_types` is accepted but DEFERRED (literal source matching only).
#[derive(Debug, Clone)]
pub struct ForbidigoOptions {
    pub forbid: Vec<ForbidigoPattern>,
    pub exclude_godoc_examples: bool,
    pub analyze_types: bool,
}

impl Default for ForbidigoOptions {
    fn default() -> Self {
        Self {
            forbid: Vec::new(),
            exclude_godoc_examples: true,
            analyze_types: false,
        }
    }
}

/// `linters.settings.varnamelen` / `linters-settings.varnamelen`.
///
/// Defaults match upstream blizzy78/varnamelen / golangci-lint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarnamelenOptions {
    /// Longest usage distance (in lines) still considered a "small" scope.
    pub max_distance: usize,
    /// Minimum name length considered "long" enough to ignore.
    pub min_name_length: usize,
    pub check_receiver: bool,
    pub check_return: bool,
    pub check_type_param: bool,
    pub ignore_type_assert_ok: bool,
    pub ignore_map_index_ok: bool,
    pub ignore_chan_recv_ok: bool,
    pub ignore_names: Vec<String>,
    pub ignore_decls: Vec<String>,
}

impl Default for VarnamelenOptions {
    fn default() -> Self {
        Self {
            max_distance: 5,
            min_name_length: 3,
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

/// `linters.settings.unparam` / `linters-settings.unparam`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnparamOptions {
    /// Inspect exported functions (golangci `check-exported`; default false).
    pub check_exported: bool,
}

impl Default for UnparamOptions {
    fn default() -> Self {
        Self {
            check_exported: false,
        }
    }
}

/// `linters.settings.unqueryvet` / `linters-settings.unqueryvet`.
///
/// Core SELECT * detection only. SQL builders / N+1 / injection / tx-leak /
/// custom DSL are DEFERRED (see DEVELOPMENT.md R13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnqueryvetOptions {
    /// Detect `SELECT t.*` patterns (upstream / golangci default: true).
    pub check_aliased_wildcard: bool,
    /// Detect `SELECT *` inside subqueries (upstream / golangci default: true).
    pub check_subqueries: bool,
    /// Regex allowlist; empty → upstream defaults (`COUNT(*)`, system catalogs, …).
    pub allowed_patterns: Vec<String>,
}

impl Default for UnqueryvetOptions {
    fn default() -> Self {
        Self {
            check_aliased_wildcard: true,
            check_subqueries: true,
            allowed_patterns: Vec::new(),
        }
    }
}

/// `linters.settings.promlinter` / `linters-settings.promlinter`.
///
/// `strict` parse-failure diagnostics are DEFERRED (accepted for config compat).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromlinterOptions {
    /// Upstream `--strict`; parse failures reported only when true (DEFERRED).
    pub strict: bool,
    /// Disable named promlint checks (`Help`, `Counter`, `CamelCase`, …).
    pub disabled_linters: Vec<String>,
}

