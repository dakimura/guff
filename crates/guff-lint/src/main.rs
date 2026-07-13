//! `guff` CLI — run bundled Go linters via one analysis pipeline.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use guff_lint::{
    discover_config, load_config, migrate_config_file, resolve_linters, run_and_print,
    standard_analyzers, ConfigError, LinterDefault, LinterSelection, LintOptions,
};

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
    /// Migrate a golangci-lint v1 config file to v2 format.
    Migrate(MigrateArgs),
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

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run(args) => match run_cmd(args) {
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
    }
}

fn run_cmd(args: RunArgs) -> Result<i32, guff_lint::RunError> {
    let selection = load_linter_selection(&args)?;
    let linter_names = selection.resolve_names();

    let mut unknown = Vec::new();
    let analyzers = if linter_names.is_empty() && args.enable.is_empty() {
        standard_analyzers()
    } else {
        resolve_linters(&linter_names, &mut |n| unknown.push(n.to_string()))
    };

    for name in &unknown {
        eprintln!("guff: linter {name:?} is not available yet");
    }

    if analyzers.is_empty() {
        eprintln!("guff: no linters to run");
        return Ok(0);
    }

    run_and_print(&LintOptions {
        patterns: args.patterns,
        analyzers,
        sequential: args.sequential,
    })
}

fn load_linter_selection(args: &RunArgs) -> Result<LinterSelection, ConfigError> {
    let file_selection = if args.no_config {
        None
    } else {
        let path = match &args.config {
            Some(p) => Some(p.clone()),
            None => discover_config(&std::env::current_dir().unwrap_or_default()),
        };
        match path {
            Some(p) => Some(load_config(&p)?),
            None => None,
        }
    };

    let base = file_selection
        .as_ref()
        .map(|c| c.linter_selection())
        .unwrap_or_default();

    let cli_default = args
        .preset
        .as_deref()
        .map(|p| {
            LinterDefault::parse(p).unwrap_or_else(|| {
                eprintln!("guff: unknown preset {p:?}, using standard");
                LinterDefault::Standard
            })
        });

    Ok(base.with_cli_overrides(cli_default, &args.enable, &args.disable))
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
        eprintln!(
            "guff: note: formatters ({}) are recorded but not run by guff yet",
            migrated.formatters.enable.join(", ")
        );
    }
    Ok(())
}
