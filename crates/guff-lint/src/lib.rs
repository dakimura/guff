//! `guff` (`guff-lint` crate) — multi-linter CLI and analyzer registry.
//!
//! Bundles golangci-lint `standard` preset linters behind a single
//! `guff_runner::run` invocation.

mod config;
mod duration;
mod exclude;
mod format;
mod nolint;
mod registry;
mod settings;

pub use config::{
    backup_path, discover_config, load_config, migrate_config_file, normalize_linter_name,
    parse_config_str, ConfigError, ConfigFile, ConfigV2, ExcludeRule, IssuesConfig, LinterDefault,
    LinterSelection, OutputConfig, RunConfig, SeverityConfig, SeverityRule, CONFIG_FILE_NAMES,
    DEPRECATED_LINTERS, FORMATTER_NAMES,
};

pub use duration::parse_go_duration;
pub use exclude::{
    default_exclude_patterns, process_diagnostics, DefaultExcludePattern, Issue, IssueFilter,
    DEFAULT_EXCLUDE_DIRS,
};
pub use format::{
    format_diagnostic_text, format_issue_text, print_issues, resolve_out_formats, Formatter,
    OutputFormatKind, TextFormatter,
};
pub use nolint::{NolintIndex, NOLINTLINT_NAME};
pub use registry::{
    analyzers_for_linter, analyzers_for_linter_with_settings, format_linters_listing,
    is_meta_linter, known_linter_names, linter_description, linter_name_for_analyzer,
    partition_linters, resolve_linters, resolve_linters_with_settings, standard_analyzers,
    KNOWN_LINTER_NAMES, STANDARD_LINTER_NAMES,
};
pub use settings::{
    ErrcheckSettings, GovetSettings, LinterSettings, StaticcheckSettings,
};

/// Package version (`CARGO_PKG_VERSION`), for `guff version`.
pub fn guff_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Default one-line version banner, matching golangci-lint's style.
pub fn version_banner() -> String {
    format!("guff has version {}", guff_version())
}

/// Exit code when `--timeout` / `run.timeout` is exceeded (golangci-lint uses 4).
pub const EXIT_TIMEOUT: i32 = 4;

/// Default `run.timeout` when neither CLI nor config set one (golangci-lint default).
pub const DEFAULT_TIMEOUT: &str = "1m";

use std::io::{self, Write};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use guff_analysis::{Analyzer, SettingsBag};
use guff_packages::{load, load_for_go_analysis, Config};
use guff_runner::{run_on_packages, RunnerError, RunnerOptions, RunResult};

/// Options for [`run_linters`].
#[derive(Debug, Clone)]
pub struct LintOptions {
    pub patterns: Vec<String>,
    pub analyzers: Vec<&'static Analyzer>,
    pub sequential: bool,
    /// Exit code when at least one diagnostic is found (golangci `--issues-exit-code`).
    pub issues_exit_code: i32,
    /// Passed to `go list` as `-tags=...`.
    pub build_tags: Vec<String>,
    /// Include test packages (`run.tests`).
    pub tests: bool,
    /// Post-processing filter (`issues` + `severity`).
    pub filter: IssueFilter,
    /// Per-linter settings (`linters.settings` / `linters-settings`).
    pub settings: std::sync::Arc<SettingsBag>,
    /// Whole-run timeout. [`Duration::ZERO`] / `None` means no timeout.
    pub timeout: Option<Duration>,
    /// Requested concurrency (`-j` / `run.concurrency`).
    ///
    /// `Some(1)` forces sequential. Values `> 1` are accepted for CLI/config
    /// compatibility; true multi-core parallel execution is DEFERRED to R9.
    pub concurrency: Option<usize>,
    /// Output formats (`--out-format`, default `[Text]`).
    pub out_formats: Vec<OutputFormatKind>,
}

impl LintOptions {
    pub fn standard(patterns: Vec<String>) -> Self {
        Self {
            patterns,
            analyzers: standard_analyzers(),
            sequential: false,
            issues_exit_code: 1,
            build_tags: Vec::new(),
            tests: false,
            filter: IssueFilter::default(),
            settings: std::sync::Arc::new(SettingsBag::default()),
            timeout: Some(Duration::from_secs(60)),
            concurrency: None,
            out_formats: vec![OutputFormatKind::Text],
        }
    }
}

/// Load packages and run analyzers. Returns diagnostics and non-zero exit hint.
pub fn run_linters(opts: &LintOptions) -> Result<LintResult, RunnerError> {
    let mut build_flags = Vec::new();
    if !opts.build_tags.is_empty() {
        build_flags.push(format!("-tags={}", opts.build_tags.join(",")));
    }
    let cfg = Config {
        mode: load_for_go_analysis(),
        build_flags,
        tests: opts.tests,
        ..Config::default()
    };
    let packages = load(&cfg, &opts.patterns).map_err(RunnerError::Load)?;
    // concurrency == 1 forces sequential; true multi-core parallel is DEFERRED (R9).
    let sequential = opts.sequential || opts.concurrency == Some(1);
    let result = run_on_packages(
        &opts.analyzers,
        &packages,
        &RunnerOptions {
            sequential,
            settings: std::sync::Arc::clone(&opts.settings),
            ..RunnerOptions::default()
        },
    )
    .map_err(RunnerError::Validate)?;
    Ok(LintResult {
        packages,
        run: result,
        filter: opts.filter.clone(),
    })
}

/// Output from [`run_linters`].
pub struct LintResult {
    pub packages: Vec<std::sync::Arc<guff_packages::Package>>,
    pub run: RunResult,
    pub filter: IssueFilter,
}

impl LintResult {
    /// Issues after applying the configured post-processing filter.
    pub fn issues(&self) -> Vec<Issue> {
        let fset = self
            .packages
            .iter()
            .find_map(|p| p.fset.as_ref())
            .expect("package missing fset");
        process_diagnostics(
            fset,
            &self.run.diagnostics(),
            &self.filter,
            &self.packages,
        )
    }

    pub fn diagnostic_count(&self) -> usize {
        self.issues().len()
    }

    /// Unfiltered diagnostic count from the runner (before exclude pipeline).
    pub fn raw_diagnostic_count(&self) -> usize {
        self.run.diagnostics().len()
    }

    /// Exit code after a successful run: `issues_exit_code` if any diagnostic, else 0.
    pub fn exit_code(&self, issues_exit_code: i32) -> i32 {
        if self.diagnostic_count() > 0 {
            issues_exit_code
        } else {
            0
        }
    }

    /// Print filtered diagnostics using `formats` (default: text). Returns the number of issues.
    pub fn print_with(
        &self,
        formats: &[OutputFormatKind],
        out: &mut dyn Write,
    ) -> io::Result<usize> {
        let issues = self.issues();
        print_issues(formats, &issues, out)
    }

    /// Print filtered diagnostics in text format to `out`. Returns the number printed.
    pub fn print_text(&self, out: &mut dyn Write) -> io::Result<usize> {
        self.print_with(&[OutputFormatKind::Text], out)
    }
}

/// Run linters and print text diagnostics to stdout.
///
/// Returns [`LintOptions::issues_exit_code`] if any diagnostic, otherwise 0.
/// Internal errors are returned as [`Err`] (caller maps them to exit code 2).
pub fn run_and_print(opts: &LintOptions) -> Result<i32, RunError> {
    run_and_write(opts, &mut io::stdout())
}

/// Like [`run_and_print`], but writes diagnostics to `out` (for tests / custom sinks).
pub fn run_and_write(opts: &LintOptions, out: &mut dyn Write) -> Result<i32, RunError> {
    if opts.analyzers.is_empty() {
        eprintln!("guff: no analyzers enabled (missing linter crates?)");
        return Ok(0);
    }

    match opts.timeout {
        Some(t) if !t.is_zero() => run_and_write_with_timeout(opts, out, t),
        _ => run_and_write_inner(opts, out),
    }
}

fn run_and_write_inner(opts: &LintOptions, out: &mut dyn Write) -> Result<i32, RunError> {
    let result = run_linters(opts)?;
    result
        .print_with(&opts.out_formats, out)
        .map_err(RunError::Io)?;
    Ok(result.exit_code(opts.issues_exit_code))
}

/// Run on a worker thread and abort the process-visible wait when `timeout` elapses.
///
/// The worker cannot be killed from Rust; on timeout we return [`RunError::Timeout`]
/// so the CLI can exit with [`EXIT_TIMEOUT`]. The OS reclaims the thread when the
/// process exits.
fn run_and_write_with_timeout(
    opts: &LintOptions,
    out: &mut dyn Write,
    timeout: Duration,
) -> Result<i32, RunError> {
    // Collect into a buffer on the worker so we only print on success.
    let opts = opts.clone();
    let (tx, rx) = mpsc::channel::<Result<(Vec<u8>, i32), String>>();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let result = run_and_write_inner(&opts, &mut buf);
        let _ = tx.send(match result {
            Ok(code) => Ok((buf, code)),
            Err(e) => Err(e.to_string()),
        });
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok((buf, code))) => {
            out.write_all(&buf).map_err(RunError::Io)?;
            Ok(code)
        }
        Ok(Err(msg)) => Err(RunError::Message(msg)),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(RunError::Timeout),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(RunError::Message(
                "lint worker exited without a result".into(),
            ))
        }
    }
}

/// Errors from the lint driver.
#[derive(Debug)]
pub enum RunError {
    Runner(RunnerError),
    Io(io::Error),
    Config(ConfigError),
    /// Whole-run timeout exceeded.
    Timeout,
    /// Worker finished with an error string (Io/Runner aren't Send through the channel).
    Message(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runner(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Config(e) => write!(f, "{e}"),
            Self::Timeout => write!(
                f,
                "timeout exceeded: try increasing it by passing --timeout option"
            ),
            Self::Message(m) => write!(f, "{m}"),
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
