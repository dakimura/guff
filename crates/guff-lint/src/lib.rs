//! `guff` (`guff-lint` crate) — multi-linter CLI and analyzer registry.
//!
//! Bundles golangci-lint `standard` preset linters behind a single
//! `guff_runner::run` invocation.

mod config;
mod format;
mod registry;

pub use config::{
    backup_path, discover_config, load_config, migrate_config_file, normalize_linter_name,
    parse_config_str, ConfigError, ConfigFile, ConfigV2, LinterDefault, LinterSelection,
    CONFIG_FILE_NAMES, DEPRECATED_LINTERS, FORMATTER_NAMES,
};

pub use format::format_diagnostic_text;
pub use registry::{
    analyzers_for_linter, known_linter_names, resolve_linters, standard_analyzers,
    STANDARD_LINTER_NAMES,
};

use std::io::{self, Write};

use guff_analysis::Analyzer;
use guff_packages::{load, load_for_go_analysis, Config};
use guff_runner::{run_on_packages, RunnerError, RunnerOptions, RunResult};

/// Options for [`run_linters`].
#[derive(Debug, Clone)]
pub struct LintOptions {
    pub patterns: Vec<String>,
    pub analyzers: Vec<&'static Analyzer>,
    pub sequential: bool,
}

impl LintOptions {
    pub fn standard(patterns: Vec<String>) -> Self {
        Self {
            patterns,
            analyzers: standard_analyzers(),
            sequential: false,
        }
    }
}

/// Load packages and run analyzers. Returns diagnostics and non-zero exit hint.
pub fn run_linters(opts: &LintOptions) -> Result<LintResult, RunnerError> {
    let cfg = Config {
        mode: load_for_go_analysis(),
        ..Config::default()
    };
    let packages = load(&cfg, &opts.patterns).map_err(RunnerError::Load)?;
    let result = run_on_packages(
        &opts.analyzers,
        &packages,
        &RunnerOptions {
            sequential: opts.sequential,
            ..RunnerOptions::default()
        },
    )
    .map_err(RunnerError::Validate)?;
    Ok(LintResult {
        packages,
        run: result,
    })
}

/// Output from [`run_linters`].
pub struct LintResult {
    pub packages: Vec<std::sync::Arc<guff_packages::Package>>,
    pub run: RunResult,
}

impl LintResult {
    pub fn diagnostic_count(&self) -> usize {
        self.run.diagnostics().len()
    }

    /// Print diagnostics in text format to `out`. Returns the number printed.
    pub fn print_text(&self, out: &mut dyn Write) -> io::Result<usize> {
        let mut count = 0;
        for (action_id, diag) in self.run.diagnostics() {
            let analyzer = action_id
                .split('@')
                .next()
                .unwrap_or(&action_id);
            let fset = self
                .packages
                .iter()
                .find_map(|p| p.fset.as_ref())
                .expect("package missing fset");
            writeln!(
                out,
                "{}",
                format_diagnostic_text(fset, analyzer, &diag)
            )?;
            count += 1;
        }
        Ok(count)
    }
}

/// Run linters and print text diagnostics to stderr. Exit code 1 if any diagnostic.
pub fn run_and_print(opts: &LintOptions) -> Result<i32, RunError> {
    if opts.analyzers.is_empty() {
        eprintln!("guff: no analyzers enabled (missing linter crates?)");
        return Ok(0);
    }

    let result = run_linters(opts)?;
    result
        .print_text(&mut io::stderr())
        .map_err(RunError::Io)?;
    Ok(if result.diagnostic_count() > 0 {
        1
    } else {
        0
    })
}

/// Errors from the lint driver.
#[derive(Debug)]
pub enum RunError {
    Runner(RunnerError),
    Io(io::Error),
    Config(ConfigError),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runner(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Config(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RunError {}

impl From<RunnerError> for RunError {
    fn from(value: RunnerError) -> Self {
        Self::Runner(value)
    }
}

impl From<ConfigError> for RunError {
    fn from(value: ConfigError) -> Self {
        Self::Config(value)
    }
}
