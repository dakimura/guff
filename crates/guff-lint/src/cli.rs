//! Shared CLI entry for `guff` and custom binaries built by `guff custom`.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use guff_fmt::{MetaFormatter, Runner, RunnerOptions};
use guff_runner::{cache_dir_size, clean_cache, default_cache_dir};

use crate::custom::{
    build_custom, discover_custom_config, load_custom_config, BuildCustomOptions, CustomError,
};
use crate::{
    default_stdout_format, discover_config, format_linters_listing, formats_from_output_config,
    guff_version, is_meta_linter, load_config, migrate_config_file, parse_go_duration,
    partition_linters, resolve_linters_with_settings, resolve_out_formats,
    run_and_write_with_teardown, version_banner, ConfigError, ConfigFile, FormattersV2,
    IssueFilter, LinterDefault, LinterSelection, LinterSettings, LintOptions, IssuesConfig,
    OutputSpec, PrinterOptions, RunError, SeverityConfig, Teardown, DEFAULT_TIMEOUT,
    EXIT_TIMEOUT, NOLINTLINT_NAME,
};
use crate::watch::run_watch;

#[derive(Parser)]
#[command(name = "guff", about = "Run Go linters through the guff analysis pipeline")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run enabled linters on packages.
    Run(RunArgs),
    /// Format Go source files (gofmt / gofumpt / goimports / gci / golines / swaggo).
    Fmt(FmtArgs),
    /// Migrate a golangci-lint v1 config file to v2 format.
    Migrate(MigrateArgs),
    /// Display the guff version.
    Version(VersionArgs),
    /// List enabled / disabled linters for the current configuration.
    Linters(LintersArgs),
    /// Cache control and information.
    Cache(CacheArgs),
    /// Build a custom guff binary with module plugins (golangci `custom`).
    Custom(CustomArgs),
}

#[derive(Parser)]
struct CustomArgs {
    /// Path to `.custom-gcl.yml` / `.custom-guff.yml` (default: discover).
    #[arg(short = 'c', long)]
    config: Option<PathBuf>,

    /// Verbose cargo / build logs.
    #[arg(short = 'v', long)]
    verbose: bool,
}

#[derive(Parser)]
struct FmtArgs {
    /// Paths to format (default: `.`).
    #[arg(default_value = ".")]
    paths: Vec<String>,

    /// Read config from this path (default: discover `.golangci.yml` / `.guff.yml`).
    #[arg(short = 'c', long)]
    config: Option<PathBuf>,

    /// Do not read a configuration file.
    #[arg(long)]
    no_config: bool,

    /// Enable a formatter by name (repeatable; default: config `formatters.enable`, else gofmt).
    #[arg(short = 'E', long = "enable")]
    enable: Vec<String>,

    /// Display diffs instead of rewriting files.
    #[arg(short = 'd', long = "diff")]
    diff: bool,

    /// Disable ANSI colors in `--diff` output (colors are on by default on a TTY).
    #[arg(long = "no-color")]
    no_color: bool,

    /// Read source from stdin; write formatted result to stdout.
    #[arg(long)]
    stdin: bool,
}

#[derive(Parser)]
struct RunArgs {
    /// Package patterns (default: `.`).
    #[arg(default_value = ".")]
    patterns: Vec<String>,

    /// Read config from this path (default: discover `.golangci.yml` / `.guff.yml`).
    #[arg(short = 'c', long)]
    config: Option<PathBuf>,

    /// Do not read a configuration file.
    #[arg(long)]
    no_config: bool,

    /// Preset linter set (`standard` = golangci-lint v2 default).
    #[arg(long, visible_alias = "default")]
    preset: Option<String>,

    /// Enable an additional linter by name (repeatable).
    #[arg(long = "enable")]
    enable: Vec<String>,

    /// Disable a linter from the preset (repeatable).
    #[arg(long = "disable")]
    disable: Vec<String>,

    /// Run analyzers sequentially (tests / deterministic output).
    #[arg(long)]
    sequential: bool,

    /// Exit code when at least one issue is found (default: 1).
    #[arg(long, default_value_t = 1)]
    issues_exit_code: i32,

    /// Build tags passed to `go list` (repeatable; merged with `run.build-tags`).
    #[arg(long = "build-tags")]
    build_tags: Vec<String>,

    /// Timeout for the whole run (Go duration: `1m`, `5m`, `30s`). `0` disables.
    /// Default: config `run.timeout`, else `1m`.
    #[arg(long)]
    timeout: Option<String>,

    /// Number of concurrent workers (`run.concurrency`). `1` forces sequential.
    #[arg(short = 'j', long = "concurrency")]
    concurrency: Option<usize>,

    /// Output format (repeatable). Default: `colored-line-number` on a TTY,
    /// `text` otherwise (golangci-compatible).
    /// Supported: `text`, `line-number`, `colored-line-number`, `json`,
    /// `checkstyle`, `sarif`, `tab`, `colored-tab`, `github-actions`.
    #[arg(long = "out-format", value_name = "FORMAT")]
    out_format: Vec<String>,

    /// Path display mode: `rel` (default, like golangci) or `abs`.
    #[arg(long = "path-mode", value_name = "MODE")]
    path_mode: Option<String>,

    /// Disable the persistent issues cache.
    #[arg(long = "no-cache")]
    no_cache: bool,

    /// Apply suggested fixes to source files and omit fixed issues from output.
    #[arg(long)]
    fix: bool,

    /// Stay running and re-lint when `.go` / `go.mod` files change (C-2).
    /// Keeps the package graph and issue-cache hashes in memory; does not
    /// retain type/SSA arenas between passes (idle RSS stays warm-sized).
    #[arg(long)]
    watch: bool,
}

#[derive(Parser)]
struct MigrateArgs {
    /// Config file to migrate (default: discover `.golangci.yml` / `.guff.yml`).
    #[arg(short = 'c', long)]
    config: Option<PathBuf>,

    /// Skip validation (allow re-migrating or v2 files).
    #[arg(long)]
    skip_validation: bool,
}

#[derive(Parser)]
struct VersionArgs {
    /// Display only the version number.
    #[arg(long)]
    short: bool,
}

#[derive(Parser)]
struct LintersArgs {
    /// Read config from this path (default: discover `.golangci.yml` / `.guff.yml`).
    #[arg(short = 'c', long)]
    config: Option<PathBuf>,

    /// Do not read a configuration file.
    #[arg(long)]
    no_config: bool,

    /// Preset linter set (`standard` = golangci-lint v2 default).
    #[arg(long, visible_alias = "default")]
    preset: Option<String>,

    /// Enable an additional linter by name (repeatable).
    #[arg(long = "enable")]
    enable: Vec<String>,

    /// Disable a linter from the preset (repeatable).
    #[arg(long = "disable")]
    disable: Vec<String>,
}

#[derive(Parser)]
struct CacheArgs {
    #[command(subcommand)]
    command: CacheCommand,
}

#[derive(Subcommand)]
enum CacheCommand {
    /// Remove the cache directory.
    Clean,
    /// Show cache directory and size.
    Status,
}

/// Run the guff CLI (also used by binaries produced by `guff custom`).
pub fn main() -> ExitCode {
    // A-9: wall from process entry through config+registry, before run_linters.
    let startup = Instant::now();
    guff_runner::init_rayon_global_stack();
    let cli = Cli::parse();
    match cli.command {
        Commands::Run(args) => match run_cmd(args, startup) {
            Ok(code) => ExitCode::from(code as u8),
            Err(err @ RunError::Timeout) => {
                eprintln!("guff: {err}");
                ExitCode::from(EXIT_TIMEOUT as u8)
            }
            Err(err) => {
                eprintln!("guff: {err}");
                ExitCode::from(2)
            }
        },
        Commands::Fmt(args) => match fmt_cmd(args) {
            Ok(code) => ExitCode::from(code as u8),
            Err(err) => {
                eprintln!("guff: {err}");
                ExitCode::from(2)
            }
        },
        Commands::Migrate(args) => match migrate_cmd(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("guff: {err}");
                ExitCode::from(2)
            }
        },
        Commands::Version(args) => {
            if args.short {
                println!("{}", guff_version());
            } else {
                println!("{}", version_banner());
            }
            ExitCode::SUCCESS
        }
        Commands::Linters(args) => match linters_cmd(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("guff: {err}");
                ExitCode::from(2)
            }
        },
        Commands::Cache(args) => match cache_cmd(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("guff: {err}");
                ExitCode::from(2)
            }
        },
        Commands::Custom(args) => match custom_cmd(args) {
            Ok(path) => {
                println!("{}", path.display());
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("guff: {err}");
                ExitCode::from(2)
            }
        },
    }
}

fn custom_cmd(args: CustomArgs) -> Result<PathBuf, CustomError> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let path = match args.config {
        Some(p) => p,
        None => discover_custom_config(&cwd).ok_or_else(|| {
            CustomError::Message(
                "no .custom-gcl.yml / .custom-guff.yml found (use -c)".into(),
            )
        })?,
    };
    let config = load_custom_config(&path)?;
    let config_dir = path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    build_custom(BuildCustomOptions {
        config,
        config_dir,
        verbose: args.verbose,
        build_dir: None,
    })
}

fn run_cmd(args: RunArgs, startup: Instant) -> Result<i32, RunError> {
    let loaded = load_run_config(
        args.no_config,
        args.config.as_ref(),
        args.preset.as_deref(),
        &args.enable,
        &args.disable,
    )?;
    let selection = loaded.selection;
    let linter_names = selection.resolve_names();
    let settings = &loaded.linter_settings;

    let report_unused_nolint = linter_names.iter().any(|n| n == NOLINTLINT_NAME);

    let mut unknown = Vec::new();
    let analyzers = if linter_names.is_empty() && args.enable.is_empty() {
        // Apply settings filters even on the implicit standard preset.
        resolve_linters_with_settings(
            &crate::STANDARD_LINTER_NAMES
                .iter()
                .map(|s| (*s).to_string())
                .collect::<Vec<_>>(),
            settings,
            &mut |_| {},
        )
    } else {
        let real: Vec<String> = linter_names
            .iter()
            .filter(|n| !is_meta_linter(n))
            .cloned()
            .collect();
        resolve_linters_with_settings(&real, settings, &mut |n| unknown.push(n.to_string()))
    };

    for name in &unknown {
        eprintln!("guff: linter {name:?} is not available yet");
    }

    if analyzers.is_empty() && !report_unused_nolint {
        eprintln!("guff: no linters to run");
        return Ok(0);
    }

    let mut build_tags = loaded.build_tags;
    for t in &args.build_tags {
        if !build_tags.iter().any(|x| x == t) {
            build_tags.push(t.clone());
        }
    }

    let mut filter = loaded.filter;
    filter.report_unused_nolint = report_unused_nolint;

    let timeout = resolve_timeout(args.timeout.as_deref(), loaded.timeout.as_deref())?;
    let concurrency = args.concurrency.or(loaded.concurrency);
    let sequential = args.sequential || concurrency == Some(1);

    let out_formats = if args.out_format.is_empty() {
        if loaded.out_formats.is_empty() {
            vec![default_stdout_format(io::stdout().is_terminal())]
        } else {
            loaded.out_formats
        }
    } else {
        resolve_out_formats(&args.out_format).map_err(RunError::Message)?
    };

    let formatters = build_formatter_run_config(
        &loaded.formatters,
        loaded.go_version.as_deref(),
        &args.patterns,
        args.fix,
        !args.no_cache,
    );

    let mut path_mode = loaded.path_mode;
    if let Some(raw) = args.path_mode.as_deref() {
        match crate::PathMode::parse(raw) {
            Some(m) => path_mode = m,
            None => {
                return Err(RunError::Message(format!(
                    "invalid --path-mode {raw:?}; use rel or abs"
                )));
            }
        }
    }

    if crate::debug::enabled() {
        eprintln!(
            "guff: phase startup (config+registry) {:.2}s",
            startup.elapsed().as_secs_f64()
        );
    }

    let opts = LintOptions {
        patterns: args.patterns,
        analyzers,
        sequential,
        issues_exit_code: args.issues_exit_code,
        build_tags,
        tests: loaded.tests,
        filter,
        settings: settings.to_bag(),
        timeout,
        concurrency,
        out_formats,
        printer: loaded.printer,
        use_cache: !args.no_cache,
        cache_dir: None,
        fix: args.fix,
        formatters,
        path_mode,
        path_prefix: loaded.path_prefix,
    };
    if args.watch {
        run_watch(&opts)
    } else {
        // One shot: the process exits as soon as this returns, so hand the
        // package graph and type arenas to the kernel instead of walking them.
        run_and_write_with_teardown(&opts, &mut io::stdout(), Teardown::LeakOnProcessExit)
    }
}

/// Build the `guff run` formatter-diagnostics config from `formatters` config.
/// Returns `None` when no (implemented) formatter is enabled.
fn build_formatter_run_config(
    formatters: &FormattersV2,
    go_version: Option<&str>,
    patterns: &[String],
    fix: bool,
    use_format_cache: bool,
) -> Option<crate::FormatterRunConfig> {
    let enable: Vec<String> = formatters
        .enable
        .iter()
        .filter(|n| guff_fmt::is_formatter(n))
        .cloned()
        .collect();
    if enable.is_empty() {
        return None;
    }

    let paths = resolve_format_paths(patterns);
    if paths.is_empty() {
        return None;
    }

    let mut gofumpt = formatters.gofumpt_options();
    if gofumpt.lang.is_none() {
        gofumpt.lang = go_version.filter(|s| !s.is_empty()).map(str::to_string);
    }
    gofumpt.match_golangci = !std::env::var_os("GUFF_GOFUMPT_MATCH_GOLANGCI")
        .is_some_and(|v| v == "0");

    Some(crate::FormatterRunConfig {
        enable,
        gofmt: formatters.gofmt_options(),
        gofumpt,
        goimports: formatters.goimports_options(),
        gci: formatters.gci_options(),
        golines: formatters.golines_options(),
        generated: formatters.exclusion_generated(),
        exclude_paths: formatters.exclusion_paths(),
        paths,
        fix,
        use_format_cache,
        cache_dir: None,
    })
}

/// Map `go list` patterns to filesystem roots for formatter checks.
/// `./...` → `.`, `pkg/...` → `pkg`, `.`/`pkg` unchanged. Non-existent or
/// non-path patterns (e.g. module paths) are skipped.
fn resolve_format_paths(patterns: &[String]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for pat in patterns {
        let mut p = pat.as_str();
        if let Some(stripped) = p.strip_suffix("...") {
            p = stripped.trim_end_matches('/');
        }
        let candidate = if p.is_empty() || p == "." || p == "./" {
            PathBuf::from(".")
        } else {
            PathBuf::from(p)
        };
        if candidate.exists() && !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    out
}

/// Resolve timeout: CLI > config > default `1m`. `0` disables.
fn resolve_timeout(
    cli: Option<&str>,
    config: Option<&str>,
) -> Result<Option<Duration>, RunError> {
    let raw = cli.or(config).unwrap_or(DEFAULT_TIMEOUT);
    let d = parse_go_duration(raw).map_err(|e| RunError::Message(format!("invalid --timeout: {e}")))?;
    if d.is_zero() {
        Ok(None)
    } else {
        Ok(Some(d))
    }
}

struct LoadedRun {
    selection: LinterSelection,
    filter: IssueFilter,
    build_tags: Vec<String>,
    tests: bool,
    linter_settings: LinterSettings,
    timeout: Option<String>,
    concurrency: Option<usize>,
    /// Empty means "use CLI/TTY default" ([`default_stdout_format`]).
    out_formats: Vec<OutputSpec>,
    printer: PrinterOptions,
    formatters: FormattersV2,
    go_version: Option<String>,
    path_mode: crate::PathMode,
    path_prefix: Option<String>,
}

fn load_run_config(
    no_config: bool,
    config: Option<&PathBuf>,
    preset: Option<&str>,
    enable: &[String],
    disable: &[String],
) -> Result<LoadedRun, ConfigError> {
    let file: Option<ConfigFile> = if no_config {
        None
    } else {
        let path = match config {
            Some(p) => Some(p.clone()),
            None => discover_config(&std::env::current_dir().unwrap_or_default()),
        };
        match path {
            Some(p) => Some(load_config(&p)?),
            None => None,
        }
    };

    let base = file
        .as_ref()
        .map(|c| c.linter_selection())
        .unwrap_or_default();

    let cli_default = preset.map(|p| {
        LinterDefault::parse(p).unwrap_or_else(|| {
            eprintln!("guff: unknown preset {p:?}, using standard");
            LinterDefault::Standard
        })
    });

    let selection = base.with_cli_overrides(cli_default, enable, disable);

    let (issues, severity, run, output, linter_settings) = match &file {
        Some(c) => (
            c.effective_issues(),
            c.severity().clone(),
            c.run().clone(),
            c.output().clone(),
            LinterSettings::from_yaml(c.linter_settings_raw()),
        ),
        None => (
            IssuesConfig::default(),
            SeverityConfig::default(),
            Default::default(),
            Default::default(),
            LinterSettings::default(),
        ),
    };

    // --no-config: still apply empty/default filter (no path excludes from file).
    let filter = if no_config {
        IssueFilter::from_config(
            &IssuesConfig {
                exclude_use_default: false,
                max_issues_per_linter: 0,
                max_same_issues: 0,
                exclude_dirs_use_default: Some(false),
                ..IssuesConfig::default()
            },
            &SeverityConfig::default(),
        )
    } else {
        IssueFilter::from_config(&issues, &severity)
    };

    // Config `output.formats` / `output.format` — CLI `--out-format` overrides.
    // Empty → CLI applies TTY-aware default (golangci colored-line-number on TTY).
    let out_formats = formats_from_output_config(&output.formats, output.format.as_deref());
    let printer = PrinterOptions::from_config(output.print_issued_lines, output.print_linter_name);

    let formatters = file.as_ref().map(|c| c.formatters()).unwrap_or_default();
    let go_version = run.go.clone();

    let mut path_mode = crate::PathMode::Rel;
    if let Some(raw) = output.path_mode.as_deref() {
        match crate::PathMode::parse(raw) {
            Some(m) => path_mode = m,
            None => eprintln!("guff: ignoring unknown output.path-mode {raw:?}"),
        }
    }

    Ok(LoadedRun {
        selection,
        filter,
        build_tags: run.build_tags,
        // golangci-lint default for `run.tests` is true (analyze `*_test.go`).
        tests: run.tests.unwrap_or(true),
        linter_settings,
        timeout: run.timeout,
        concurrency: run.concurrency.map(|n| n.max(0) as usize),
        out_formats,
        printer,
        formatters,
        go_version,
        path_mode,
        path_prefix: output.path_prefix.clone(),
    })
}

fn linters_cmd(args: LintersArgs) -> Result<(), ConfigError> {
    let loaded = load_run_config(
        args.no_config,
        args.config.as_ref(),
        args.preset.as_deref(),
        &args.enable,
        &args.disable,
    )?;
    let (enabled, disabled) = partition_linters(&loaded.selection);
    format_linters_listing(&enabled, &disabled, &mut std::io::stdout())
        .map_err(|e| ConfigError::Io(e))?;
    Ok(())
}

fn fmt_cmd(args: FmtArgs) -> Result<i32, RunError> {
    let (formatters, go_version) = load_formatters_config(args.no_config, args.config.as_ref())?;

    let enable = if args.enable.is_empty() {
        formatters.enable.clone()
    } else {
        args.enable.clone()
    };
    let enable = validate_formatters(&enable)?;

    let mut gofumpt = formatters.gofumpt_options();
    if gofumpt.lang.is_none() {
        gofumpt.lang = go_version.filter(|s| !s.is_empty());
    }

    let meta = MetaFormatter::new(
        &enable,
        formatters.gofmt_options(),
        gofumpt,
        formatters.goimports_options(),
        formatters.gci_options(),
        formatters.golines_options(),
    )
    .map_err(|e| RunError::Message(e.to_string()))?;

    let color = args.diff && !args.no_color && io::stdout().is_terminal();
    let runner = Runner::new(
        meta,
        RunnerOptions {
            diff: args.diff,
            stdin: args.stdin,
            exclude_paths: formatters.exclusion_paths(),
            generated: formatters.exclusion_generated(),
            color,
            format_cache: None,
        },
    );

    let paths: Vec<PathBuf> = args.paths.iter().map(PathBuf::from).collect();
    let mut stdout = io::stdout();
    let stats = runner
        .run(&paths, &mut stdout)
        .map_err(|e| RunError::Message(e.to_string()))?;
    let _ = stdout.flush();
    Ok(stats.exit_code)
}

/// Validate formatter names (all listed formatters are implemented). Empty is OK
/// (MetaFormatter falls back to gofmt).
fn validate_formatters(enable: &[String]) -> Result<Vec<String>, RunError> {
    for name in enable {
        if !guff_fmt::is_formatter(name) {
            return Err(RunError::Message(format!("invalid formatter {name:?}")));
        }
    }
    Ok(enable.to_vec())
}

/// Load `formatters` config plus the `run.go` version (for gofumpt `-lang`).
fn load_formatters_config(
    no_config: bool,
    config_path: Option<&PathBuf>,
) -> Result<(FormattersV2, Option<String>), RunError> {
    if no_config {
        return Ok((FormattersV2::default(), None));
    }
    let path = match config_path {
        Some(p) => Some(p.clone()),
        None => discover_config(&std::env::current_dir().unwrap_or_default()),
    };
    let Some(path) = path else {
        return Ok((FormattersV2::default(), None));
    };
    let cfg = load_config(&path).map_err(|e| RunError::Message(e.to_string()))?;
    let go = cfg.run().go.clone();
    Ok((cfg.formatters(), go))
}

fn migrate_cmd(args: MigrateArgs) -> Result<(), ConfigError> {
    let path = match args.config {
        Some(p) => p,
        None => discover_config(&std::env::current_dir().unwrap_or_default())
            .ok_or(ConfigError::NotFound)?,
    };

    let migrated = migrate_config_file(&path, args.skip_validation)?;
    let backup = crate::backup_path(&path);
    eprintln!(
        "guff: migrated {} -> v2 (backup: {})",
        path.display(),
        backup.display()
    );
    if !migrated.formatters.enable.is_empty() {
        let known: Vec<_> = migrated
            .formatters
            .enable
            .iter()
            .filter(|n| guff_fmt::is_formatter(n))
            .cloned()
            .collect();
        let unknown: Vec<_> = migrated
            .formatters
            .enable
            .iter()
            .filter(|n| !guff_fmt::is_formatter(n))
            .cloned()
            .collect();
        if !known.is_empty() {
            eprintln!(
                "guff: note: formatters ({}) available via `guff fmt`",
                known.join(", ")
            );
        }
        if !unknown.is_empty() {
            eprintln!(
                "guff: note: unknown formatters ({}) recorded",
                unknown.join(", ")
            );
        }
    }
    Ok(())
}

fn cache_cmd(args: CacheArgs) -> Result<(), RunError> {
    let dir = default_cache_dir().map_err(|e| RunError::Message(e.to_string()))?;
    match args.command {
        CacheCommand::Clean => {
            clean_cache(&dir).map_err(|e| RunError::Message(e.to_string()))?;
            eprintln!("guff: cleaned cache at {}", dir.display());
        }
        CacheCommand::Status => {
            let size = cache_dir_size(&dir);
            println!("Dir: {}", dir.display());
            println!("Size: {}", prettify_bytes(size));
            match guff_runner::default_go_cache_dir() {
                Ok(gocache) => {
                    println!("GOCACHE: {}", gocache.display());
                    println!("GOCACHE size: {}", prettify_bytes(cache_dir_size(&gocache)));
                }
                Err(e) => println!("GOCACHE: ({e})"),
            }
        }
    }
    Ok(())
}

fn prettify_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let n = n as f64;
    if n >= GB {
        format!("{:.1} GB", n / GB)
    } else if n >= MB {
        format!("{:.1} MB", n / MB)
    } else if n >= KB {
        format!("{:.1} KB", n / KB)
    } else {
        format!("{n} B")
    }
}
