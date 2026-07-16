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
}

impl Default for GoconstOptions {
    fn default() -> Self {
        Self {
            min_len: 3,
            min_occurrences: 3,
            ignore_calls: true,
            ignore_tests: false,
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
