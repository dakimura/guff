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
}

impl Default for CyclopOptions {
    fn default() -> Self {
        Self {
            max_complexity: 10,
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
}

impl Default for NakedretOptions {
    fn default() -> Self {
        Self {
            max_func_lines: 30,
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
