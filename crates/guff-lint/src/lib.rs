//! `guff` (`guff-lint` crate) — multi-linter CLI and analyzer registry.
//!
//! Bundles golangci-lint `standard` preset linters behind a single
//! `guff_runner::run` invocation.

mod config;
mod duration;
mod exclude;
mod fix;
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
    default_exclude_patterns, issue_from_cached, process_diagnostics, DefaultExcludePattern, Issue,
    IssueFilter, DEFAULT_EXCLUDE_DIRS,
};
pub use format::{
    format_diagnostic_text, format_issue_text, print_issues, resolve_out_formats,
    CheckstyleFormatter, Formatter, GithubActionsFormatter, JsonFormatter, JsonReport,
    JsonWarning, OutputFormatKind, SarifFormatter, TabFormatter, TextFormatter,
};
pub use nolint::{NolintIndex, NOLINTLINT_NAME};
pub use registry::{
    analyzers_for_linter, analyzers_for_linter_with_settings, format_linters_listing,
    is_meta_linter, known_linter_names, linter_description, linter_name_for_analyzer,
    partition_linters, resolve_linters, resolve_linters_with_settings, standard_analyzers,
    KNOWN_LINTER_NAMES, STANDARD_LINTER_NAMES,
};
pub use fix::{apply_fixes, FixError};
pub use settings::{
    ErrcheckSettings, ErrchkjsonSettings, GovetSettings, LinterSettings, ReviveRuleSetting,
    ReviveSettings, StaticcheckSettings,
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
use guff_packages::{
    load_for_go_analysis, load_graph, typecheck_roots, Config, LoadMode, TypecheckEnv,
};
use guff_runner::{
    build_salt, default_cache_dir, detect_go_version, run_on_packages, HashMode, IssueCache,
    RunnerError, RunnerOptions, RunResult,
};

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
    /// `Some(1)` forces sequential. Values `> 1` (or `None` with available
    /// parallelism) size the runner's rayon thread pool.
    pub concurrency: Option<usize>,
    /// Output formats (`--out-format`, default `[Text]`).
    pub out_formats: Vec<OutputFormatKind>,
    /// Use persistent issues cache (default true). Disable with `--no-cache`.
    pub use_cache: bool,
    /// Override cache directory (`GUFF_CACHE` / `GOLANGCI_LINT_CACHE` otherwise).
    pub cache_dir: Option<std::path::PathBuf>,
    /// Apply the first suggested fix for each diagnostic to source files (`--fix`).
    pub fix: bool,
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
            use_cache: true,
            cache_dir: None,
            fix: false,
        }
    }
}

/// Load packages and run analyzers. Returns diagnostics and non-zero exit hint.
pub fn run_linters(opts: &LintOptions) -> Result<LintResult, RunnerError> {
    let mut build_flags = Vec::new();
    if !opts.build_tags.is_empty() {
        build_flags.push(format!("-tags={}", opts.build_tags.join(",")));
    }
    let sequential = opts.sequential || opts.concurrency == Some(1);

    // Lazy load: first resolve package *metadata only* (`go list`, no parsing or
    // type-checking). This is enough to compute issues-cache keys and decide
    // which packages actually need work. Full analysis mode is kept separately
    // for the packages that miss the cache.
    let analysis_mode = load_for_go_analysis();
    // Metadata mode = analysis mode minus the parse/type-check bits. Enough for
    // `go list` + cache-key computation; no source is parsed or type-checked.
    let metadata_mode = LoadMode::NEED_NAME
        | LoadMode::NEED_FILES
        | LoadMode::NEED_COMPILED_GO_FILES
        | LoadMode::NEED_IMPORTS
        | LoadMode::NEED_DEPS
        | LoadMode::NEED_EXPORT_FILE;
    let meta_cfg = Config {
        mode: metadata_mode,
        build_flags: build_flags.clone(),
        tests: opts.tests,
        ..Config::default()
    };
    let full_cfg = Config {
        mode: analysis_mode,
        build_flags,
        tests: opts.tests,
        ..Config::default()
    };

    let (roots, all_packages) =
        load_graph(&meta_cfg, &opts.patterns).map_err(RunnerError::Load)?;

    // Build the cache with a complete dependency-hash registry over *all* loaded
    // packages (roots + transitive deps) so `NeedAllDeps` hashing is
    // deterministic and warm runs hit reliably.
    let cache = if opts.use_cache {
        open_issue_cache(opts).map(|mut c| {
            if let Err(err) = c.set_dep_hashes(&all_packages) {
                eprintln!("guff: cache dep-hash registry failed ({err})");
            }
            std::sync::Arc::new(c)
        })
    } else {
        None
    };

    // Partition roots into cache hits (issues restored from disk — no parsing)
    // and misses (need type-checking + analysis).
    let mut cached_issues: Vec<Issue> = Vec::new();
    let mut miss_ids: Vec<String> = Vec::new();
    let mut hit_roots: Vec<std::sync::Arc<guff_packages::Package>> = Vec::new();
    let mut hits = 0usize;
    for root in &roots {
        let restored = cache
            .as_ref()
            .and_then(|c| c.get_cached(root, HashMode::NeedAllDeps).ok());
        match restored {
            Some(diags) => {
                hits += 1;
                hit_roots.push(std::sync::Arc::clone(root));
                for d in diags {
                    let action_id = format!("{}@{}", d.analyzer, root.pkg_path);
                    cached_issues.push(issue_from_cached(
                        action_id.split('@').next().unwrap_or(&d.analyzer),
                        &d.filename,
                        d.line,
                        d.column,
                        &d.message,
                        &d.category,
                        &d.url,
                        &d.severity,
                    ));
                }
            }
            None => miss_ids.push(root.id.clone()),
        }
    }

    // Type-check + analyze only the packages that missed the cache.
    let env = TypecheckEnv::from_env(&full_cfg.resolved_env(), "gc");
    let miss_roots = typecheck_roots(&all_packages, &miss_ids, analysis_mode, &env);

    let result = run_on_packages(
        &opts.analyzers,
        &miss_roots,
        &RunnerOptions {
            sequential,
            concurrency: opts.concurrency,
            settings: std::sync::Arc::clone(&opts.settings),
            cache,
            ..RunnerOptions::default()
        },
    )
    .map_err(RunnerError::Validate)?;

    if std::env::var_os("GUFF_DEBUG_CACHE").is_some() {
        eprintln!(
            "guff: cache hits={} misses={} (lazy: type-checked {} of {} roots)",
            hits,
            miss_ids.len(),
            miss_roots.len(),
            roots.len(),
        );
    }

    // Package list for output/nolint: type-checked misses (carry the `FileSet`
    // for fresh diagnostics) plus metadata-only hits (supply source paths).
    let mut packages = miss_roots;
    packages.extend(hit_roots);

    Ok(LintResult {
        packages,
        run: result,
        filter: opts.filter.clone(),
        cached_issues,
    })
}

fn open_issue_cache(opts: &LintOptions) -> Option<IssueCache> {
    let dir = match &opts.cache_dir {
        Some(p) => p.clone(),
        None => match default_cache_dir() {
            Ok(p) => p,
            Err(_) => return None,
        },
    };
    let mut names: Vec<&str> = opts.analyzers.iter().map(|a| a.name).collect();
    names.sort_unstable();
    let mut settings_fp = format!("keys={:?}", opts.settings);
    if let Some(ec) = opts.settings.get::<guff_errcheck::Options>("errcheck") {
        settings_fp.push_str(&format!(" errcheck={ec:?}"));
    }
    let salt = build_salt(
        guff_version(),
        &names,
        &opts.build_tags,
        &settings_fp,
        &detect_go_version(),
    );
    match IssueCache::open(dir, salt) {
        Ok(c) => Some(c),
        Err(err) => {
            eprintln!("guff: cache disabled ({err})");
            None
        }
    }
}

/// Output from [`run_linters`].
pub struct LintResult {
    pub packages: Vec<std::sync::Arc<guff_packages::Package>>,
    pub run: RunResult,
    pub filter: IssueFilter,
    /// Issues restored from the persistent cache for packages that were not
    /// re-analyzed. Positions are already resolved (no `FileSet` needed).
    pub cached_issues: Vec<Issue>,
}

impl LintResult {
    /// Issues after applying the configured post-processing filter.
    pub fn issues(&self) -> Vec<Issue> {
        // Cache-restored issues carry resolved positions already. Freshly
        // analyzed diagnostics (cache misses) are resolved against the shared
        // `FileSet` of the type-checked packages. Both streams then go through
        // the same filter pipeline (exclude rules, //nolint, severity, limits).
        let mut issues = self.cached_issues.clone();
        if let Some(fset) = self.packages.iter().find_map(|p| p.fset.as_ref()) {
            issues.extend(IssueFilter::collect_issues(fset, &self.run.diagnostics()));
        }
        self.filter.apply(issues, &self.packages)
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

    /// Filtered issues, optionally applying suggested fixes to disk.
    pub fn issues_and_fix(&self, apply_fix: bool) -> Result<(Vec<Issue>, usize), FixError> {
        let issues = self.issues();
        if !apply_fix {
            return Ok((issues, 0));
        }
        let Some(fset) = self.packages.iter().find_map(|p| p.fset.as_ref()) else {
            return Ok((issues, 0));
        };
        apply_fixes(fset, &issues)
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
    let (issues, fixes_applied) = result.issues_and_fix(opts.fix)?;
    if fixes_applied > 0 {
        eprintln!("guff: fixed {fixes_applied} issue(s)");
    }
    print_issues(&opts.out_formats, &issues, out).map_err(RunError::Io)?;
    Ok(if issues.is_empty() {
        0
    } else {
        opts.issues_exit_code
    })
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

impl From<FixError> for RunError {
    fn from(value: FixError) -> Self {
        Self::Message(value.to_string())
    }
}
