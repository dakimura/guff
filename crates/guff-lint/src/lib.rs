//! `guff` (`guff-lint` crate) — multi-linter CLI and analyzer registry.
//!
//! Bundles golangci-lint `standard` preset linters behind a single
//! `guff_runner::run` invocation.

pub mod cli;
mod config;
mod custom;
mod debug;
mod diff;
mod duration;
mod exclude;
mod fix;
mod format;
mod nolint;
pub mod nolintlint;
mod pathutil;
mod registry;
mod settings;
mod watch;

pub use config::{
    backup_path, discover_config, load_config, migrate_config_file, normalize_linter_name,
    parse_config_str, parse_gofmt_settings, validate_gocritic_options, ConfigError, ConfigFile,
    ConfigV2, ExcludeRule,
    FormatterExclusions, FormattersV2, IssuesConfig, LinterDefault, LinterExclusions,
    LinterSelection, OutputConfig, RunConfig, SeverityConfig, SeverityRule, CONFIG_FILE_NAMES,
    DEPRECATED_LINTERS, FORMATTER_NAMES,
};

pub use custom::{
    build_custom, discover_custom_config, generate_custom_project, load_custom_config,
    parse_custom_config, resolve_guff_src, BuildCustomOptions, CustomError, CustomGclConfig,
    CustomPluginEntry, CUSTOM_CONFIG_NAMES,
};

pub use duration::parse_go_duration;
pub use exclude::{
    default_exclude_patterns, issue_from_cached, process_diagnostics, DefaultExcludePattern, Issue,
    IssueFilter, DEFAULT_EXCLUDE_DIRS,
};
pub use format::{
    default_stdout_format, format_diagnostic_text, format_issue_text, formats_from_output_config,
    print_issues, print_issues_with, resolve_out_formats, CheckstyleFormatter, Formatter,
    GithubActionsFormatter, JsonFormatter, JsonReport, JsonWarning, OutputFormatKind, OutputSpec,
    PrinterOptions, SarifFormatter, TabFormatter, TextFormatter,
};
pub use nolint::{NolintIndex, NOLINTLINT_NAME};
pub use nolintlint::NolintlintStyle;
pub use pathutil::{format_issue_path, PathMode};
pub use registry::{
    analyzers_for_linter, analyzers_for_linter_with_settings, format_linters_listing,
    is_meta_linter, known_linter_names, linter_description, linter_name_for_analyzer,
    partition_linters, resolve_linters, resolve_linters_with_settings, standard_analyzers,
    KNOWN_LINTER_NAMES, STANDARD_LINTER_NAMES,
};
pub use fix::{apply_fixes, FixError};
pub use settings::{
    BodycloseSettings, CustomLinterConfig, DepguardDenySetting, DepguardRuleSetting,
    DepguardSettings, DupwordSettings, ErrcheckSettings, ErrchkjsonSettings, FuncorderSettings,
    GodoclintSettings, GodotSettings, GodoxSettings, GomoddirectivesSettings, GomodguardSettings,
    GovetSettings, LinterSettings, ReviveRuleSetting, ReviveSettings, RowserrcheckSettings,
    StaticcheckSettings, VarnamelenSettings, WrapcheckSettings,
};

/// golangci-lint version this release targets for config / finding-set parity.
///
/// guff uses its own SemVer; bump this when the compatibility pin moves.
pub const GOLANGCI_LINT_COMPAT: &str = "2.12.2";

/// Package version (`CARGO_PKG_VERSION`), for `guff version --short`.
pub fn guff_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Default one-line version banner, matching golangci-lint's style plus compat pin.
pub fn version_banner() -> String {
    format!(
        "guff has version {} (golangci-lint compat {})",
        guff_version(),
        GOLANGCI_LINT_COMPAT
    )
}

/// Exit code when the configuration selects nothing to run — no linters and no
/// formatters (golangci-lint's "Running error: no linters enabled" uses 3).
pub const EXIT_NO_LINTERS: i32 = 3;

/// Exit code when guff refuses to start on the given configuration
/// (golangci-lint's `exitcodes.Failure`). Same value as [`EXIT_NO_LINTERS`];
/// named separately because "I will not run this config" and "this config runs
/// nothing" are different answers to the user.
pub const EXIT_CONFIG_ERROR: i32 = 3;

/// Exit code when `--timeout` / `run.timeout` is exceeded (golangci-lint uses 4).
pub const EXIT_TIMEOUT: i32 = 4;

/// Default `run.timeout` when neither CLI nor config set one (golangci-lint default).
pub const DEFAULT_TIMEOUT: &str = "1m";

use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::sync::mpsc;
use std::time::Duration;

use guff_analysis::{Analyzer, SettingsBag};
use guff_packages::{
    load_for_go_analysis, load_graph, typecheck_roots_with_prebuilt_seed, Config, LoadMode,
    TypecheckEnv, start_seed_speculation,
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
    /// Canonical fingerprint of the raw `linters.settings` YAML, used in the
    /// issues-cache salt.
    ///
    /// [`SettingsBag`] is type-erased, so its `Debug` can only name the keys —
    /// fingerprinting that alone made every settings *value* invisible to the
    /// cache, and `guff run` served stale diagnostics after a config edit.
    /// Empty means "no linter settings" (or a caller that never set it), which
    /// simply salts as the empty string.
    pub settings_fingerprint: String,
    /// Whole-run timeout. [`Duration::ZERO`] / `None` means no timeout.
    pub timeout: Option<Duration>,
    /// Requested concurrency (`-j` / `run.concurrency`).
    ///
    /// `Some(1)` forces sequential. Values `> 1` (or `None` with available
    /// parallelism) size the runner's rayon thread pool.
    pub concurrency: Option<usize>,
    /// Output formats (`--out-format`, default `[Text]` on stdout).
    /// Each entry may include a file path (`format:path`).
    pub out_formats: Vec<OutputSpec>,
    /// golangci `output.print-issued-lines` / `output.print-linter-name`.
    pub printer: PrinterOptions,
    /// Use persistent issues cache (default true). Disable with `--no-cache`.
    pub use_cache: bool,
    /// Override cache directory (`GUFF_CACHE` / `GOLANGCI_LINT_CACHE` otherwise).
    pub cache_dir: Option<std::path::PathBuf>,
    /// Apply the first suggested fix for each diagnostic to source files (`--fix`).
    pub fix: bool,
    /// Optional formatter diagnostics for `guff run` (golangci `formatters`).
    /// When set, unformatted files are reported as issues (or fixed with `fix`).
    pub formatters: Option<FormatterRunConfig>,
    /// golangci `output.path-mode` (default: relative to cwd).
    pub path_mode: PathMode,
    /// golangci `output.path-prefix`.
    pub path_prefix: Option<String>,
}

/// Formatter configuration for `guff run` diagnostics (golangci `formatters`).
///
/// Held as plain data (all fields `Clone`) so [`LintOptions`] stays cloneable;
/// the formatters are built lazily during the run.
#[derive(Debug, Clone)]
pub struct FormatterRunConfig {
    /// Implemented formatter names in `enable` order (gofmt/gofumpt/…/swaggo).
    pub enable: Vec<String>,
    pub gofmt: guff_fmt::GofmtOptions,
    pub gofumpt: guff_fmt::GofumptOptions,
    pub goimports: guff_fmt::GoimportsOptions,
    pub gci: guff_fmt::GciOptions,
    pub golines: guff_fmt::GolinesOptions,
    pub generated: guff_fmt::GeneratedMode,
    /// `formatters.exclusions.paths`.
    pub exclude_paths: Vec<String>,
    /// Filesystem roots derived from the run patterns.
    pub paths: Vec<std::path::PathBuf>,
    /// Rewrite files in place instead of reporting (`--fix`).
    pub fix: bool,
    /// Enable `${GUFF_CACHE}/fmt_check` warm cache (off with `--no-cache`).
    pub use_format_cache: bool,
    /// Explicit cache root (same as issue cache). `None` → env / default.
    pub cache_dir: Option<std::path::PathBuf>,
    /// golangci `run.tests` — when false, skip `*_test.go` format diagnostics.
    pub include_tests: bool,
    /// golangci `run.build-tags` for build-constraint filtering.
    pub build_tags: Vec<String>,
}

/// Check (or fix) formatting for `guff run`; returns issues for unformatted files.
///
/// Each enabled formatter is checked independently (matching golangci, which
/// runs one analyzer per formatter), so issues are attributed to the specific
/// formatter. With `fix`, files are rewritten via the chained formatters and no
/// issues are produced.
fn run_format_checks(cfg: &FormatterRunConfig, filter: &IssueFilter) -> Result<Vec<Issue>, RunError> {
    run_format_checks_inner(cfg, Some(filter))
}

/// Format findings without the exclude/nolint pipeline.
///
/// Callers that merge format issues with analysis diagnostics before a single
/// [`IssueFilter::apply`] (so `//nolint:gofumpt` marks correctly) use this.
fn run_format_checks_raw(cfg: &FormatterRunConfig) -> Result<Vec<Issue>, RunError> {
    run_format_checks_inner(cfg, None)
}

fn run_format_checks_inner(
    cfg: &FormatterRunConfig,
    filter: Option<&IssueFilter>,
) -> Result<Vec<Issue>, RunError> {
    use guff_fmt::{
        format_cache_dir_from_env, FormatCheckCache, MetaFormatter, Runner, RunnerOptions,
    };

    if cfg.enable.is_empty() {
        return Ok(Vec::new());
    }

    let format_cache = if cfg.use_format_cache {
        let dir = format_cache_dir_from_env()
            .or_else(|| cfg.cache_dir.clone())
            .or_else(|| default_cache_dir().ok());
        dir.and_then(|d| FormatCheckCache::open(d).ok())
    } else {
        None
    };

    if cfg.fix {
        let meta = MetaFormatter::new(
            &cfg.enable,
            cfg.gofmt.clone(),
            cfg.gofumpt.clone(),
            cfg.goimports.clone(),
            cfg.gci.clone(),
            cfg.golines.clone(),
        )
        .map_err(|e| RunError::Message(e.to_string()))?;
        let runner = Runner::new(
            meta,
            RunnerOptions {
                exclude_paths: cfg.exclude_paths.clone(),
                generated: cfg.generated,
                include_tests: cfg.include_tests,
                build_tags: cfg.build_tags.clone(),
                filter_build_constraints: true,
                ..Default::default()
            },
        );
        let mut sink = io::sink();
        runner
            .run(&cfg.paths, &mut sink)
            .map_err(|e| RunError::Message(e.to_string()))?;
        return Ok(Vec::new());
    }

    // One tree walk + one read per file for all formatters (B-10). Attribution
    // stays per-formatter (golangci-style). Native gci+gofumpt also share a
    // skip-object parse inside `check_files_multi`.
    let detail = crate::debug::detailed();
    let t_collect = std::time::Instant::now();
    let files = Runner::collect_paths(&cfg.paths).map_err(|e| RunError::Message(e.to_string()))?;
    if detail {
        eprintln!(
            "guff:     format collect_paths {:.2}s ({} files, {} roots)",
            t_collect.elapsed().as_secs_f64(),
            files.len(),
            cfg.paths.len(),
        );
    }
    let meta = MetaFormatter::new(
        &cfg.enable,
        cfg.gofmt.clone(),
        cfg.gofumpt.clone(),
        cfg.goimports.clone(),
        cfg.gci.clone(),
        cfg.golines.clone(),
    )
    .map_err(|e| RunError::Message(e.to_string()))?;
    let formatters = meta.into_formatters();
    let runner_opts = RunnerOptions {
        exclude_paths: cfg.exclude_paths.clone(),
        generated: cfg.generated,
        format_cache: format_cache.clone(),
        include_tests: cfg.include_tests,
        build_tags: cfg.build_tags.clone(),
        filter_build_constraints: true,
        ..Default::default()
    };
    let t_fmt = std::time::Instant::now();
    let findings = guff_fmt::check_files_multi(&formatters, &files, &runner_opts)
        .map_err(|e| RunError::Message(e.to_string()))?;
    if detail {
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for f in &findings {
            *counts.entry(f.formatter.as_str()).or_default() += 1;
        }
        let summary: Vec<String> = cfg
            .enable
            .iter()
            .map(|n| format!("{n}={}", counts.get(n.as_str()).copied().unwrap_or(0)))
            .collect();
        eprintln!(
            "guff:     format multi {:.2}s ({} formatters; unformatted {})",
            t_fmt.elapsed().as_secs_f64(),
            formatters.len(),
            summary.join(", "),
        );
        for line in guff_fmt::format_stage_report() {
            eprintln!("{line}");
        }
    }
    let mut issues = Vec::new();
    for f in findings {
        issues.push(issue_from_cached(
            &f.formatter,
            &f.file,
            f.line,
            1,
            "File is not properly formatted",
            "",
            "",
            // golangci-lint leaves a formatter issue's severity empty — gosec
            // is the only linter it attaches one to by default. Hardcoding
            // "error" here made every formatter finding differ from upstream in
            // a field the finding-set diff does not key on, so only the
            // check-level golden could see it.
            "",
        ));
    }
    match filter {
        Some(f) => Ok(f.apply(issues, &[])),
        None => Ok(issues),
    }
}

/// Hybrid cold path is on by default. `GUFF_DEP_SOURCE=0` / `false` / `off` opts
/// out to the legacy export-data dependency seed; any other value (or unset)
/// keeps hybrid enabled.
fn dep_source_enabled() -> bool {
    match std::env::var("GUFF_DEP_SOURCE") {
        Ok(v) => {
            let v = v.trim();
            !(v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off"))
        }
        Err(_) => true,
    }
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
            settings_fingerprint: String::new(),
            filter: IssueFilter::default(),
            settings: std::sync::Arc::new(SettingsBag::default()),
            timeout: Some(Duration::from_secs(60)),
            concurrency: None,
            out_formats: vec![OutputSpec::new(OutputFormatKind::Text)],
            printer: PrinterOptions::default(),
            use_cache: true,
            cache_dir: None,
            fix: false,
            formatters: None,
            path_mode: PathMode::Rel,
            path_prefix: None,
        }
    }
}

/// Analyzers that read `ast::Ident.obj` (filled by parser object resolution).
///
/// When none of them is enabled, target parse can set
/// [`TypecheckEnv::skip_object_resolution`] (P0-3) and skip the walk.
///
/// **An analyzer missing from this list does not fail — it goes quiet.** The
/// field is simply `None` for every identifier, so whatever the analyzer asks
/// of it answers "no". `testinggoroutine` was written, tested against the
/// golden tier, and found short by exactly one finding for this reason: the one
/// shape whose region is reached through `Ident.obj` (`fn := func(){…}; go fn()`).
const AST_OBJECT_RESOLUTION_ANALYZERS: &[&str] =
    &["ineffassign", "maintidx", "testinggoroutine"];

fn analyzers_need_ast_object_resolution(analyzers: &[&Analyzer]) -> bool {
    analyzers
        .iter()
        .any(|a| AST_OBJECT_RESOLUTION_ANALYZERS.contains(&a.name))
}

/// Load packages and run analyzers. Returns diagnostics and non-zero exit hint.
pub fn run_linters(opts: &LintOptions) -> Result<LintResult, RunnerError> {
    guff_runner::init_rayon_global_stack();
    let prepared = prepare_linter_run(opts)?;
    run_linters_on_graph(opts, &prepared.graph, prepared.cache, prepared.speculate_job)
}

/// Metadata-only package graph (`go list`), shared by one-shot and `--watch`.
#[derive(Clone)]
pub struct MetadataGraph {
    pub roots: Vec<std::sync::Arc<guff_packages::Package>>,
    pub all_packages: Vec<std::sync::Arc<guff_packages::Package>>,
}

pub(crate) struct PreparedLint {
    pub(crate) graph: MetadataGraph,
    pub(crate) cache: Option<std::sync::Arc<IssueCache>>,
    pub(crate) speculate_job: Option<guff_packages::SpeculativeSeedJob>,
}

fn metadata_and_full_cfg(opts: &LintOptions) -> (Config, Config, LoadMode, bool, bool) {
    let mut build_flags = Vec::new();
    if !opts.build_tags.is_empty() {
        build_flags.push(format!("-tags={}", opts.build_tags.join(",")));
    }
    let sequential = opts.sequential || opts.concurrency == Some(1);
    let analysis_mode = load_for_go_analysis();
    let metadata_mode = LoadMode::NEED_NAME
        | LoadMode::NEED_FILES
        | LoadMode::NEED_COMPILED_GO_FILES
        | LoadMode::NEED_IMPORTS
        | LoadMode::NEED_DEPS
        | LoadMode::NEED_EXPORT_FILE
        | LoadMode::NEED_MODULE;
    let dep_source = dep_source_enabled();
    let meta_cfg = Config {
        mode: metadata_mode,
        build_flags: build_flags.clone(),
        tests: opts.tests,
        disable_cache: !opts.use_cache,
        dep_source,
        ..Config::default()
    };
    let full_cfg = Config {
        mode: analysis_mode,
        build_flags,
        tests: opts.tests,
        disable_cache: !opts.use_cache,
        dep_source,
        ..Config::default()
    };
    (meta_cfg, full_cfg, analysis_mode, dep_source, sequential)
}

pub(crate) fn prepare_linter_run(opts: &LintOptions) -> Result<PreparedLint, RunnerError> {
    let (meta_cfg, _full_cfg, _analysis_mode, dep_source, sequential) = metadata_and_full_cfg(opts);
    let timing = crate::debug::enabled();
    let t0 = std::time::Instant::now();

    let mut speculate_env = TypecheckEnv::from_env(&meta_cfg.resolved_env(), "gc");
    speculate_env.from_source = dep_source;
    speculate_env.parallel = !sequential;
    speculate_env.skip_object_resolution =
        !analyzers_need_ast_object_resolution(&opts.analyzers);
    let speculate_job = if !opts.use_cache && dep_source {
        start_seed_speculation(&meta_cfg, &opts.patterns, &speculate_env)
    } else {
        None
    };

    let (roots, all_packages) =
        load_graph(&meta_cfg, &opts.patterns).map_err(RunnerError::Load)?;
    if timing {
        eprintln!(
            "guff: phase load_graph (go list) {:.2}s ({} roots, {} total pkgs)",
            t0.elapsed().as_secs_f64(),
            roots.len(),
            all_packages.len(),
        );
    }

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

    Ok(PreparedLint {
        graph: MetadataGraph {
            roots,
            all_packages,
        },
        cache,
        speculate_job,
    })
}

/// Analyze a preloaded metadata graph (used by one-shot and `--watch` re-runs).
///
/// `speculate_job` is only set on the first cold `--no-cache` path; watch
/// re-runs pass `None`.
pub(crate) fn run_linters_on_graph(
    opts: &LintOptions,
    graph: &MetadataGraph,
    cache: Option<std::sync::Arc<IssueCache>>,
    speculate_job: Option<guff_packages::SpeculativeSeedJob>,
) -> Result<LintResult, RunnerError> {
    let (_, full_cfg, analysis_mode, dep_source, sequential) = metadata_and_full_cfg(opts);
    let timing = crate::debug::enabled();
    let t1 = std::time::Instant::now();
    let roots = &graph.roots;
    let all_packages = &graph.all_packages;

    // Partition roots into cache hits (issues restored from disk — no parsing)
    // and misses (need type-checking + analysis). Parallelize lookups: each hit
    // path hashes the package's sources + reads a JSON entry (warm ~0.1s serial).
    let mut cached_issues: Vec<Issue> = Vec::new();
    let mut miss_ids: Vec<String> = Vec::new();
    let mut hit_roots: Vec<std::sync::Arc<guff_packages::Package>> = Vec::new();
    let mut hits = 0usize;

    enum Part {
        Empty,
        Hit(Vec<guff_runner::CachedDiagnostic>),
        Miss,
    }

    let parts: Vec<Part> = {
        use rayon::prelude::*;
        roots
            .par_iter()
            .map(|root| {
                if root.compiled_go_files.is_empty() {
                    return Part::Empty;
                }
                match cache
                    .as_ref()
                    .and_then(|c| c.get_cached(root, HashMode::NeedAllDeps).ok())
                {
                    Some(diags) => Part::Hit(diags),
                    None => Part::Miss,
                }
            })
            .collect()
    };

    for (root, part) in roots.iter().zip(parts) {
        match part {
            Part::Empty => {
                hits += 1;
                hit_roots.push(std::sync::Arc::clone(root));
            }
            Part::Hit(diags) => {
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
            Part::Miss => {
                if timing {
                    eprintln!(
                        "guff:   cache miss root id={} path={}",
                        root.id, root.pkg_path
                    );
                }
                miss_ids.push(root.id.clone());
            }
        }
    }

    if timing {
        eprintln!(
            "guff: phase cache setup+partition {:.2}s ({} hits, {} misses)",
            t1.elapsed().as_secs_f64(),
            hits,
            miss_ids.len(),
        );
    }
    let t2 = std::time::Instant::now();

    // Type-check + analyze only the packages that missed the cache.
    // contextcheck needs same-module imported packages typechecked so package
    // facts can be computed — otherwise cross-pkg findings are silently dropped
    // (helm). Other fact producers (modernize, deprecated, …) do not need this
    // expansion; gating on any `fact_types` inflated peak RSS by hundreds of MB
    // on prometheus without changing findings.
    let root_miss_ids = miss_ids.clone();
    let need_fact_pkgs = analyzers_need_same_module_fact_packages(&opts.analyzers);
    let typecheck_ids = if need_fact_pkgs {
        expand_fact_typecheck_ids(all_packages, &root_miss_ids)
    } else {
        root_miss_ids.clone()
    };
    let mut env = TypecheckEnv::from_env(&full_cfg.resolved_env(), "gc");
    env.from_source = dep_source;
    env.parallel = !sequential;
    env.skip_object_resolution = !analyzers_need_ast_object_resolution(&opts.analyzers);
    let prebuilt = speculate_job.and_then(|job| {
        job.finish_if_matches(all_packages, &typecheck_ids)
            .map(|s| (s.seed, s.fset))
    });
    let mut typed =
        typecheck_roots_with_prebuilt_seed(all_packages, &typecheck_ids, analysis_mode, &env, prebuilt);
    // The runner resolves an import stub to the type-checked package of the same
    // id through this table. Handing it the packages instead of rewriting each
    // one's `imports` keeps `Package`'s `Vec<File>` of syntax un-cloned: doing
    // the rewrite cost +36% peak RSS on jaeger, and doing it non-transitively —
    // every import pointing at a pre-rewrite instance whose own imports were
    // still stubs — did not even work.
    let (miss_roots, typed_by_id): (
        Vec<std::sync::Arc<guff_packages::Package>>,
        Option<std::sync::Arc<HashMap<String, std::sync::Arc<guff_packages::Package>>>>,
    ) = if need_fact_pkgs {
        let by_id: HashMap<String, std::sync::Arc<guff_packages::Package>> = typed
            .iter()
            .map(|p| (p.id.clone(), std::sync::Arc::clone(p)))
            .collect();
        let roots = root_miss_ids
            .iter()
            .filter_map(|id| by_id.get(id).cloned())
            .collect();
        (roots, Some(std::sync::Arc::new(by_id)))
    } else {
        (typed, None)
    };
    if timing {
        eprintln!(
            "guff: phase typecheck_roots {:.2}s ({} pkgs, {} analyze roots)",
            t2.elapsed().as_secs_f64(),
            typecheck_ids.len(),
            miss_roots.len(),
        );
    }
    guff_packages::report_process("post typecheck_roots");
    guff_packages::report_packages("post typecheck_roots", &miss_roots);
    crate::debug::report_rss_after_collect("post typecheck_roots");
    let t3 = std::time::Instant::now();

    let result = run_on_packages(
        &opts.analyzers,
        &miss_roots,
        &RunnerOptions {
            sequential,
            concurrency: opts.concurrency,
            settings: std::sync::Arc::clone(&opts.settings),
            cache,
            typed_by_id,
            ..RunnerOptions::default()
        },
    )
    .map_err(RunnerError::Validate)?;
    if timing {
        eprintln!(
            "guff: phase analyze (run_on_packages) {:.2}s",
            t3.elapsed().as_secs_f64(),
        );
    }
    guff_packages::report_process("post analyze");
    guff_packages::report_packages("post analyze", &miss_roots);
    crate::debug::report_rss_after_collect("post analyze");

    if crate::debug::enabled() {
        eprintln!(
            "guff: cache hits={} misses={} (lazy: type-checked {} of {} roots)",
            hits,
            root_miss_ids.len(),
            miss_roots.len(),
            roots.len(),
        );
    }

    let mut packages = miss_roots;
    packages.extend(hit_roots);

    Ok(LintResult {
        packages,
        run: result,
        filter: opts.filter.clone(),
        cached_issues,
        path_mode: opts.path_mode,
        path_prefix: opts.path_prefix.clone(),
    })
}

/// Whether any enabled analyzer needs same-module import packages typechecked
/// so its package facts can be produced (kkHAIKE/contextcheck `getPkgRoot`).
///
/// Do **not** key this off arbitrary `fact_types`: modernize / deprecated /
/// exhaustive advertise facts but do not need the same-module source expansion,
/// and expanding for them regresses peak RSS on large corpora (prometheus).
pub(crate) fn analyzers_need_same_module_fact_packages(
    analyzers: &[&guff_analysis::Analyzer],
) -> bool {
    fn needs(a: &guff_analysis::Analyzer) -> bool {
        a.name == "contextcheck" || a.requires.iter().any(|r| needs(r))
    }
    analyzers.iter().any(|a| needs(a))
}

/// Module path used to bound fact-package typecheck expansion (kkHAIKE/contextcheck
/// `getPkgRoot` / same-module facts). Prefer `Package.module.path`.
fn package_module_key(pkg: &guff_packages::Package) -> String {
    if let Some(m) = pkg.module.as_ref() {
        if !m.path.is_empty() {
            return m.path.clone();
        }
    }
    let parts: Vec<&str> = pkg.pkg_path.split('/').collect();
    if parts.len() < 3 || !parts[0].contains('.') {
        parts.first().copied().unwrap_or("").to_string()
    } else {
        parts[..3].join("/")
    }
}

/// Extend cache-miss root ids with same-module imported packages that have
/// source files, so contextcheck fact producers can run on them.
fn expand_fact_typecheck_ids(
    all_packages: &[std::sync::Arc<guff_packages::Package>],
    miss_ids: &[String],
) -> Vec<String> {
    let by_id: HashMap<&str, &std::sync::Arc<guff_packages::Package>> = all_packages
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();
    let mut out: Vec<String> = miss_ids.to_vec();
    let mut seen: HashSet<String> = miss_ids.iter().cloned().collect();
    let mut stack = miss_ids.to_vec();
    let root_modules: HashSet<String> = miss_ids
        .iter()
        .filter_map(|id| by_id.get(id.as_str()).map(|p| package_module_key(p)))
        .collect();
    while let Some(id) = stack.pop() {
        let Some(pkg) = by_id.get(id.as_str()) else {
            continue;
        };
        for dep in pkg.imports.values() {
            if dep.compiled_go_files.is_empty() {
                continue;
            }
            if !root_modules.contains(&package_module_key(dep)) {
                continue;
            }
            // The import names the plain `P`, which is exactly the id
            // `filter_duplicate_packages` removes when `P [P.test]` is loaded
            // too. Type-checking an id that is no longer in `all_packages`
            // produces nothing at all, so ask for the variant that is there.
            let dep_id = if by_id.contains_key(dep.id.as_str()) {
                dep.id.clone()
            } else {
                let variant = guff_packages::same_package_test_variant_id(&dep.id);
                if !by_id.contains_key(variant.as_str()) {
                    continue;
                }
                variant
            };
            if seen.insert(dep_id.clone()) {
                out.push(dep_id.clone());
                stack.push(dep_id);
            }
        }
    }
    out
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
    // Keys alone are not enough: two runs that enable the same linters with
    // different settings must not share cache entries.
    let settings_fp = format!(
        "keys={:?} raw={}",
        opts.settings, opts.settings_fingerprint
    );
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
    pub path_mode: PathMode,
    pub path_prefix: Option<String>,
}

impl LintResult {
    /// Diagnostics before exclude / `//nolint` / severity / limits.
    ///
    /// Positions are resolved; paths are still absolute (filters need that).
    pub fn unfiltered_issues(&self) -> Vec<Issue> {
        let mut issues = self.cached_issues.clone();
        // Each analyze root may share a FileSet Arc after typecheck_roots, but
        // empty packages get a private empty FileSet. Resolve each diagnostic
        // against the producing package's fset (`analyzer@pkg_path`) so positions
        // from later roots are not dropped or remapped through the wrong set.
        let mut fsets = std::collections::HashMap::<&str, &guff::position::FileSet>::new();
        for pkg in &self.packages {
            if let Some(fs) = pkg.fset.as_ref() {
                fsets.insert(pkg.pkg_path.as_str(), fs);
            }
        }
        for (action_id, diag) in self.run.diagnostics() {
            let pkg_path = action_id.split('@').nth(1).unwrap_or("");
            let Some(fset) = fsets.get(pkg_path) else {
                continue;
            };
            issues.extend(IssueFilter::collect_issues(fset, &[(action_id, diag)]));
        }
        issues
    }

    /// Apply the configured post-processing filter and path display mode.
    pub fn filter_issues(&self, mut issues: Vec<Issue>) -> Vec<Issue> {
        issues = self.filter.apply(issues, &self.packages);
        let prefix = self.path_prefix.as_deref();
        for issue in &mut issues {
            issue.filename = format_issue_path(&issue.filename, self.path_mode, prefix);
        }
        issues
    }

    /// Issues after applying the configured post-processing filter.
    pub fn issues(&self) -> Vec<Issue> {
        // Cache-restored issues carry resolved positions already. Freshly
        // analyzed diagnostics are resolved against each producing package's
        // FileSet (see [`Self::unfiltered_issues`]). Both streams then go through
        // the same filter pipeline (exclude rules, //nolint, severity, limits).
        self.filter_issues(self.unfiltered_issues())
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
        formats: &[OutputSpec],
        out: &mut dyn Write,
    ) -> io::Result<usize> {
        self.print_with_options(formats, &PrinterOptions::default(), out)
    }

    /// Print filtered diagnostics with golangci `output.print-*` options.
    pub fn print_with_options(
        &self,
        formats: &[OutputSpec],
        opts: &PrinterOptions,
        out: &mut dyn Write,
    ) -> io::Result<usize> {
        let issues = self.issues();
        print_issues_with(formats, opts, &issues, out)
    }

    /// Print filtered diagnostics in text format to `out`. Returns the number printed.
    pub fn print_text(&self, out: &mut dyn Write) -> io::Result<usize> {
        self.print_with(&[OutputSpec::new(OutputFormatKind::Text)], out)
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

/// What to do with the analysis artifacts once the diagnostics are printed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Teardown {
    /// Free the package graph, syntax trees and type arenas before returning.
    Free,
    /// Leave them allocated because the process is about to exit.
    ///
    /// A cold `./...` sweep over Prometheus keeps ~3.7 GiB of packages, ASTs and
    /// type arenas reachable from [`LintResult`]. Walking that graph to free it
    /// costs **0.28s single-threaded** (docs/PERF_TASKS_V2.md §A-10) and happens
    /// *after* the last diagnostic has been written, so a one-shot CLI pays it as
    /// pure latency for memory the kernel is about to reclaim wholesale.
    ///
    /// Only correct when nothing runs after the call. Anything long-lived
    /// (`--watch`, tests, embedders) must use [`Teardown::Free`].
    LeakOnProcessExit,
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
    run_and_write_with_teardown(opts, out, Teardown::Free)
}

/// Like [`run_and_write`], but lets a process that exits next skip the teardown.
pub fn run_and_write_with_teardown(
    opts: &LintOptions,
    out: &mut dyn Write,
    teardown: Teardown,
) -> Result<i32, RunError> {
    // nolintlint alone enables zero analyzers but still needs package load +
    // unused-directive reporting (golangci parity). So does a format-only
    // config (`linters.default: none` with `formatters.enable`), which
    // golangci-lint runs normally.
    if opts.analyzers.is_empty()
        && opts.filter.nolintlint.is_none()
        && opts.formatters.is_none()
    {
        eprintln!("guff: no analyzers enabled (missing linter crates?)");
        return Ok(0);
    }

    match opts.timeout {
        Some(t) if !t.is_zero() => run_and_write_with_timeout(opts, out, t, teardown),
        _ => run_and_write_inner(opts, out, teardown),
    }
}

/// Number of threads for the private format-check rayon pool.
///
/// The default is 2, and that is not arbitrary: format checks fully *overlap*
/// the `go list` + typecheck + analyze window, so the phase never sits on the
/// critical path. A cold `./...` sweep on a 10-core host (docs/PERF_TASKS_V2.md
/// P0-1) shows wall is minimized at 2 threads (4.50s) and rises as the pool
/// grows (4→4.73s, 10→4.80s): extra format threads finish the phase sooner
/// (fmt 1.6s→0.6s) but only add CPU contention to the analysis that actually
/// bounds the run. Seed-hot behaves the same (wall flat ~3.46s from 2 threads
/// up). So 2 stays.
///
/// Private rayon pool size for overlapped format checks.
///
/// P0-1 (2026-07-27) found that with analysis ≈ 4s, format was fully overlapped
/// and raising the count only stole CPU from typecheck — default stayed 2.
/// After C-3c / B-10 (2026-07-31) analysis is ~1.1–1.9s and seed-hot
/// `format_checks waited` is ~0.5–0.6s with 2 threads, so format is on the
/// critical path. Remeasured sweep (prometheus `./...`, 10-core arm64):
///
/// | threads | seed-hot wall (med) | empty-cold wall (med) |
/// |---:|---:|---:|
/// | 2 | 1.69s | 2.14s |
/// | 3 | 1.24s | 2.03s |
/// | 4 | 1.24s | 2.01s |
/// | 6 | 1.23s | 2.01s |
///
/// Default is `(ncpu/3).clamp(2, 4)` (10-core → 3). `GUFF_FMT_THREADS` still
/// overrides. With `-j 1` / sequential analysis, format also uses 1 thread.
fn fmt_thread_count(concurrency: Option<usize>, sequential: bool) -> usize {
    if let Some(v) = std::env::var_os("GUFF_FMT_THREADS") {
        if let Some(n) = v.to_str().and_then(|s| s.trim().parse::<usize>().ok()) {
            return n.max(1);
        }
    }
    if sequential || concurrency == Some(1) {
        return 1;
    }
    let ncpu = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    (ncpu / 3).clamp(2, 4)
}

fn run_and_write_inner(
    opts: &LintOptions,
    out: &mut dyn Write,
    teardown: Teardown,
) -> Result<i32, RunError> {
    let timing = crate::debug::enabled();

    // Formatting reads files straight from disk and needs neither the package
    // graph nor types, while `run_linters` opens with a `go list` subprocess
    // that leaves the cores idle for over a second. Start the checks now and
    // collect them below, so they cost wall time only if they outlast the
    // analysis. `--fix` stays sequential: it rewrites the very files the
    // analysis is reading, and it runs after the analysis' own fixes.
    let fmt_threads = fmt_thread_count(
        opts.concurrency,
        opts.sequential || opts.concurrency == Some(1),
    );
    let fmt_job = opts
        .formatters
        .as_ref()
        .filter(|cfg| !cfg.fix && !opts.fix)
        .map(|cfg| {
            let cfg = cfg.clone();
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                // Format checks run native Rust formatters over every file via
                // rayon; pin them to a private pool so they don't steal the
                // whole global pool from typecheck/analyze. Pool size: see
                // `fmt_thread_count` (raised after C-3c made format critical
                // on seed-hot).
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(fmt_threads)
                    .thread_name(|i| format!("guff-fmt-{i}"))
                    .build()
                    .expect("format rayon pool");
                // Raw findings: merged with analysis before one filter.apply so
                // //nolint:gofumpt (etc.) marks as used (golangci parity).
                let result = pool.install(|| run_format_checks_raw(&cfg));
                (result, started.elapsed())
            })
        });

    let result = run_linters(opts)?;
    let tf = std::time::Instant::now();
    let mut issues = result.unfiltered_issues();
    let mut fmt_ran = None;
    let mut fmt_waited = None;
    if let Some(job) = fmt_job {
        let twait = std::time::Instant::now();
        let (fmt_result, ran) = job.join().unwrap_or_else(|p| std::panic::resume_unwind(p));
        fmt_waited = Some(twait.elapsed());
        fmt_ran = Some(ran);
        issues.extend(fmt_result?);
    }
    let (issues, fixes_applied) = {
        let filtered = result.filter_issues(issues);
        if opts.fix {
            if let Some(fset) = result.packages.iter().find_map(|p| p.fset.as_ref()) {
                apply_fixes(fset, &filtered)?
            } else {
                (filtered, 0)
            }
        } else {
            (filtered, 0)
        }
    };
    if timing {
        eprintln!("guff: phase issues+filter {:.2}s", tf.elapsed().as_secs_f64());
        if let (Some(ran), Some(waited)) = (fmt_ran, fmt_waited) {
            eprintln!(
                "guff: phase format_checks {:.2}s (overlapped with analysis; {:.2}s waited)",
                ran.as_secs_f64(),
                waited.as_secs_f64(),
            );
        }
    }
    if fixes_applied > 0 {
        eprintln!("guff: fixed {fixes_applied} issue(s)");
    }
    // `--fix` (or formatter fix mode): rewrite files after analysis; no issues.
    if opts.fix || opts.formatters.as_ref().is_some_and(|c| c.fix) {
        if let Some(fmt_cfg) = &opts.formatters {
            let tfmt = std::time::Instant::now();
            let _ = run_format_checks(fmt_cfg, &opts.filter)?;
            if timing {
                eprintln!("guff: phase format_checks {:.2}s", tfmt.elapsed().as_secs_f64());
            }
        }
    }
    let tp = std::time::Instant::now();
    print_issues_with(&opts.out_formats, &opts.printer, &issues, out).map_err(RunError::Io)?;
    if timing {
        eprintln!("guff: phase print {:.2}s", tp.elapsed().as_secs_f64());
    }
    let code = if issues.is_empty() {
        0
    } else {
        opts.issues_exit_code
    };
    let td = std::time::Instant::now();
    if rss_category_probe_enabled() {
        rss_category_probe(result);
    } else {
        match teardown {
            Teardown::Free => drop(result),
            Teardown::LeakOnProcessExit => std::mem::forget(result),
        }
    }
    if timing {
        eprintln!(
            "guff: phase teardown {:.2}s ({})",
            td.elapsed().as_secs_f64(),
            match teardown {
                Teardown::Free => "freed",
                Teardown::LeakOnProcessExit => "left to process exit",
            },
        );
    }
    Ok(code)
}

/// Whether `GUFF_DEBUG_RSS=3` asked for the destructive category probe.
fn rss_category_probe_enabled() -> bool {
    std::env::var_os("GUFF_DEBUG_RSS").is_some_and(|v| v.to_str() == Some("3"))
}

/// Take the run's retained memory apart one category at a time, printing RSS
/// after each, so the categories are *measured* rather than estimated.
///
/// `rss::attribute_packages` estimates: a flat 192 bytes per AST node, arena
/// slots without the heap hanging off each type, `Info` maps by entry count. It
/// names 1.29 GiB of a 2.17 GiB process on prometheus `./...` and says so, but
/// an estimate cannot tell you whether the missing 0.88 GiB is one structure
/// nobody counted or every estimate being 40% low (PERF_TASKS_V6 §4.1).
/// Dropping a category and reading RSS back answers that exactly.
///
/// Destructive and debug-only: it runs after issues are printed, in place of
/// the teardown that would otherwise leak the result to process exit.
fn rss_category_probe(mut result: LintResult) {
    guff_packages::report_process("teardown start");
    // Every action holds an `Arc<Package>`; until the graph is gone the package
    // Arcs are shared and cannot be taken apart in place.
    result.run.graph = guff_runner::Graph::empty();
    result.run.packages.clear();
    guff_packages::report_process("after dropping the action graph");

    let mut shared = 0usize;
    let mut with_syntax = 0usize;
    for pkg in &mut result.packages {
        match std::sync::Arc::get_mut(pkg) {
            Some(p) => {
                if !p.syntax.is_empty() {
                    with_syntax += 1;
                }
                p.syntax = Vec::new();
                p.source_files = Vec::new();
            }
            None => shared += 1,
        }
    }
    guff_packages::report_process(&format!(
        "after dropping syntax + source bytes ({with_syntax} pkgs, {shared} still shared)"
    ));

    for pkg in &mut result.packages {
        if let Some(p) = std::sync::Arc::get_mut(pkg) {
            p.types_info = None;
        }
    }
    guff_packages::report_process("after dropping Info maps");

    for pkg in &mut result.packages {
        if let Some(p) = std::sync::Arc::get_mut(pkg) {
            p.type_artifacts = None;
        }
    }
    guff_packages::report_process("after dropping type artifacts (arenas)");

    for pkg in &mut result.packages {
        if let Some(p) = std::sync::Arc::get_mut(pkg) {
            p.fset = None;
            p.imports.clear();
            p.deps = Vec::new();
        }
    }
    guff_packages::report_process("after dropping FileSet + import graph + dep lists");

    drop(result);
    guff_packages::report_process("after dropping everything");
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
    teardown: Teardown,
) -> Result<i32, RunError> {
    // Collect into a buffer on the worker so we only print on success.
    let opts = opts.clone();
    const LINT_WORKER_STACK: usize = 8 * 1024 * 1024;
    let (tx, rx) = mpsc::channel::<Result<(Vec<u8>, i32), String>>();
    std::thread::Builder::new()
        .stack_size(LINT_WORKER_STACK)
        .spawn(move || {
            let mut buf = Vec::new();
            let result = run_and_write_inner(&opts, &mut buf, teardown);
            let _ = tx.send(match result {
                Ok(code) => Ok((buf, code)),
                Err(e) => Err(e.to_string()),
            });
        })
        .map_err(|e| RunError::Message(format!("failed to spawn lint worker: {e}")))?;

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

#[cfg(test)]
mod format_check_tests {
    use super::*;

    fn cfg(paths: Vec<std::path::PathBuf>, fix: bool) -> FormatterRunConfig {
        FormatterRunConfig {
            enable: vec!["gofmt".to_string()],
            gofmt: guff_fmt::GofmtOptions::default(),
            gofumpt: guff_fmt::GofumptOptions::default(),
            goimports: guff_fmt::GoimportsOptions::default(),
            gci: guff_fmt::GciOptions::default(),
            golines: guff_fmt::GolinesOptions::default(),
            generated: guff_fmt::GeneratedMode::Lax,
            exclude_paths: Vec::new(),
            paths,
            fix,
            use_format_cache: false,
            cache_dir: None,
            include_tests: true,
            build_tags: Vec::new(),
        }
    }

    #[test]
    fn analyzers_need_ast_object_resolution_gates_p0_3() {
        assert!(!analyzers_need_ast_object_resolution(&[]));
        let printf = guff_govet::analyzers()
            .into_iter()
            .find(|a| a.name == "printf")
            .expect("printf");
        assert!(!analyzers_need_ast_object_resolution(&[printf]));
        assert!(analyzers_need_ast_object_resolution(&[
            guff_ineffassign::analyzer()
        ]));
        assert!(analyzers_need_ast_object_resolution(&[guff_style::maintidx()]));
        assert!(analyzers_need_ast_object_resolution(&[
            printf,
            guff_ineffassign::analyzer(),
        ]));
    }

    #[test]
    fn reports_unformatted_file_as_issue() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.go");
        std::fs::write(&path, "package main\nfunc main(  ) {\n}\n").unwrap();

        let issues =
            run_format_checks(&cfg(vec![path.clone()], false), &IssueFilter::default()).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].from_linter, "gofmt");
        assert_eq!(issues[0].text, "File is not properly formatted");
        // File is untouched in check mode.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "package main\nfunc main(  ) {\n}\n"
        );
    }

    #[test]
    fn fix_rewrites_and_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.go");
        std::fs::write(&path, "package main\nfunc main(  ) {\n}\n").unwrap();

        let issues =
            run_format_checks(&cfg(vec![path.clone()], true), &IssueFilter::default()).unwrap();
        assert!(issues.is_empty());
        let got = std::fs::read_to_string(&path).unwrap();
        assert!(got.contains("func main() {"), "not fixed:\n{got}");
    }

    #[test]
    fn formatted_file_yields_no_issue() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.go");
        std::fs::write(&path, "package main\n\nfunc main() {}\n").unwrap();
        let issues =
            run_format_checks(&cfg(vec![path], false), &IssueFilter::default()).unwrap();
        assert!(issues.is_empty(), "unexpected: {issues:?}");
    }
}
