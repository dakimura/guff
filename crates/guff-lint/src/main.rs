//! `guff` CLI — run bundled Go linters via one analysis pipeline.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use guff_fmt::{MetaFormatter, Runner, RunnerOptions};
use guff_lint::{
    discover_config, format_linters_listing, formats_from_output_config, guff_version,
    is_meta_linter, load_config, migrate_config_file, parse_go_duration, partition_linters,
    resolve_linters_with_settings, resolve_out_formats, run_and_print, version_banner,
    ConfigError, ConfigFile, FormattersV2, IssueFilter, LinterDefault, LinterSelection,
    LinterSettings, LintOptions, IssuesConfig, OutputSpec, RunError, SeverityConfig,
    DEFAULT_TIMEOUT, EXIT_TIMEOUT, NOLINTLINT_NAME,
};
use guff_runner::{cache_dir_size, clean_cache, default_cache_dir};

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
    /// Format Go source files (gofmt; more formatters → R15).
    Fmt(FmtArgs),
    /// Migrate a golangci-lint v1 config file to v2 format.
    Migrate(MigrateArgs),
    /// Display the guff version.
    Version(VersionArgs),
    /// List enabled / disabled linters for the current configuration.
    Linters(LintersArgs),
    /// Cache control and information.
    Cache(CacheArgs),
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

    /// Output format (repeatable). Default: `text`.
    /// Supported: `text`, `line-number`, `colored-line-number`, `json`,
    /// `checkstyle`, `sarif`, `tab`, `colored-tab`, `github-actions`.
    #[arg(long = "out-format", value_name = "FORMAT")]
    out_format: Vec<String>,

    /// Disable the persistent issues cache.
    #[arg(long = "no-cache")]
    no_cache: bool,

    /// Apply suggested fixes to source files and omit fixed issues from output.
    #[arg(long)]
    fix: bool,
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

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run(args) => match run_cmd(args) {
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
    }
}

fn run_cmd(args: RunArgs) -> Result<i32, RunError> {
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
            &guff_lint::STANDARD_LINTER_NAMES
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
        loaded.out_formats
    } else {
        resolve_out_formats(&args.out_format).map_err(RunError::Message)?
    };

    run_and_print(&LintOptions {
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
        use_cache: !args.no_cache,
        cache_dir: None,
        fix: args.fix,
    })
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
    out_formats: Vec<OutputSpec>,
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
    let out_formats = formats_from_output_config(&output.formats, output.format.as_deref());

    Ok(LoadedRun {
        selection,
        filter,
        build_tags: run.build_tags,
        tests: run.tests.unwrap_or(false),
        linter_settings,
        timeout: run.timeout,
        concurrency: run.concurrency.map(|n| n.max(0) as usize),
        out_formats,
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
    let formatters = load_formatters_config(args.no_config, args.config.as_ref())?;

    let enable = if args.enable.is_empty() {
        formatters.enable.clone()
    } else {
        args.enable.clone()
    };

    // When config enables unimplemented formatters (goimports/…), prefer CLI
    // `--enable gofmt`/`gofumpt` or fall back if at least one implemented remains.
    let enable = filter_implemented_formatters(&enable)?;

    let meta = MetaFormatter::new(
        &enable,
        formatters.gofmt_options(),
        formatters.gofumpt_options(),
    )
    .map_err(|e| RunError::Message(e.to_string()))?;

    let runner = Runner::new(
        meta,
        RunnerOptions {
            diff: args.diff,
            stdin: args.stdin,
            exclude_paths: formatters.exclusion_paths(),
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

/// Keep only implemented formatters; error if the set is non-empty and none remain.
fn filter_implemented_formatters(enable: &[String]) -> Result<Vec<String>, RunError> {
    if enable.is_empty() {
        return Ok(Vec::new()); // MetaFormatter defaults to gofmt
    }
    const IMPLEMENTED: &[&str] = &["gofmt", "gofumpt"];
    let mut kept = Vec::new();
    let mut deferred = Vec::new();
    for name in enable {
        if IMPLEMENTED.contains(&name.as_str()) {
            kept.push(name.clone());
        } else if guff_fmt::is_formatter(name) {
            deferred.push(name.clone());
        } else {
            return Err(RunError::Message(format!("invalid formatter {name:?}")));
        }
    }
    if kept.is_empty() && !deferred.is_empty() {
        return Err(RunError::Message(format!(
            "formatter(s) {:?} are not implemented yet (use -E gofmt or -E gofumpt, or wait for R15 follow-up)",
            deferred
        )));
    }
    for name in &deferred {
        eprintln!(
            "guff fmt: note: formatter {name:?} is not implemented yet; skipping (DEFERRED → R15)"
        );
    }
    Ok(kept)
}

fn load_formatters_config(
    no_config: bool,
    config_path: Option<&PathBuf>,
) -> Result<FormattersV2, RunError> {
    if no_config {
        return Ok(FormattersV2::default());
    }
    let path = match config_path {
        Some(p) => Some(p.clone()),
        None => discover_config(&std::env::current_dir().unwrap_or_default()),
    };
    let Some(path) = path else {
        return Ok(FormattersV2::default());
    };
    let cfg = load_config(&path).map_err(|e| RunError::Message(e.to_string()))?;
    Ok(cfg.formatters())
}

fn migrate_cmd(args: MigrateArgs) -> Result<(), ConfigError> {
    let path = match args.config {
        Some(p) => p,
        None => discover_config(&std::env::current_dir().unwrap_or_default())
            .ok_or(ConfigError::NotFound)?,
    };

    let migrated = migrate_config_file(&path, args.skip_validation)?;
    let backup = guff_lint::backup_path(&path);
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
            .filter(|n| *n == "gofmt")
            .cloned()
            .collect();
        let deferred: Vec<_> = migrated
            .formatters
            .enable
            .iter()
            .filter(|n| *n != "gofmt")
            .cloned()
            .collect();
        if !known.is_empty() {
            eprintln!(
                "guff: note: formatters ({}) available via `guff fmt`",
                known.join(", ")
            );
        }
        if !deferred.is_empty() {
            eprintln!(
                "guff: note: formatters ({}) are recorded but not implemented yet",
                deferred.join(", ")
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
