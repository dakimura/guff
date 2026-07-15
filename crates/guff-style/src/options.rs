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
