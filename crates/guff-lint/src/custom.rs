//! `guff custom` — build a custom guff binary with module plugins.
//!
//! Compatible with golangci-lint's `.custom-gcl.yml` workflow.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// Build-config file names (golangci first).
pub const CUSTOM_CONFIG_NAMES: &[&str] = &[".custom-gcl.yml", ".custom-guff.yml"];

/// Parsed `.custom-gcl.yml` / `.custom-guff.yml`.
#[derive(Debug, Clone, Deserialize)]
pub struct CustomGclConfig {
    /// Informational guff version (mismatch → warning only).
    #[serde(default)]
    pub version: Option<String>,
    /// Output binary name (default: `custom-guff`).
    #[serde(default = "default_name")]
    pub name: String,
    /// Directory for the binary (default: `.`).
    #[serde(default = "default_destination")]
    pub destination: String,
    #[serde(default)]
    pub plugins: Vec<CustomPluginEntry>,
}

fn default_name() -> String {
    "custom-guff".into()
}

fn default_destination() -> String {
    ".".into()
}

impl Default for CustomGclConfig {
    fn default() -> Self {
        Self {
            version: None,
            name: default_name(),
            destination: default_destination(),
            plugins: Vec::new(),
        }
    }
}

/// One plugin in `.custom-gcl.yml`.
#[derive(Debug, Clone, Deserialize)]
pub struct CustomPluginEntry {
    /// Module / crate identity (Go module path or crate name).
    pub module: String,
    /// Rust crate to link (defaults to last segment of `module`).
    #[serde(default)]
    pub import: Option<String>,
    /// Local filesystem path to the plugin crate.
    #[serde(default)]
    pub path: Option<String>,
    /// Git tag / version when `path` is absent.
    #[serde(default)]
    pub version: Option<String>,
}

/// Errors from `guff custom`.
#[derive(Debug)]
pub enum CustomError {
    Io(std::io::Error),
    Message(String),
    Cargo(String),
}

impl std::fmt::Display for CustomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Message(m) | Self::Cargo(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for CustomError {}

impl From<std::io::Error> for CustomError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Discover `.custom-gcl.yml` then `.custom-guff.yml` walking up from `start`.
pub fn discover_custom_config(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        for name in CUSTOM_CONFIG_NAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Parse a custom-gcl YAML document.
pub fn parse_custom_config(contents: &str) -> Result<CustomGclConfig, CustomError> {
    serde_yaml::from_str(contents).map_err(|e| CustomError::Message(format!("parse config: {e}")))
}

/// Load custom config from a path.
pub fn load_custom_config(path: &Path) -> Result<CustomGclConfig, CustomError> {
    let contents = fs::read_to_string(path)?;
    parse_custom_config(&contents)
}

/// Resolve the guff workspace root (contains `crates/guff-lint`).
pub fn resolve_guff_src() -> Result<PathBuf, CustomError> {
    if let Ok(src) = std::env::var("GUFF_SRC") {
        let p = PathBuf::from(src);
        if p.join("crates/guff-lint").is_dir() {
            return Ok(p);
        }
        return Err(CustomError::Message(format!(
            "GUFF_SRC={} does not look like a guff workspace (missing crates/guff-lint)",
            p.display()
        )));
    }

    // Embedded at compile time of this guff binary (dev / source builds).
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/guff-lint → workspace root
    if let Some(root) = manifest.parent().and_then(|p| p.parent()) {
        if root.join("crates/guff-lint").is_dir() {
            return Ok(root.to_path_buf());
        }
    }

    Err(CustomError::Message(
        "cannot locate guff source: set GUFF_SRC to the workspace root, \
         or run a guff binary built from this repository"
            .into(),
    ))
}

fn cargo_package_name(entry: &CustomPluginEntry) -> String {
    let raw = entry
        .import
        .as_deref()
        .unwrap_or(entry.module.as_str());
    let last = raw.rsplit('/').next().unwrap_or(raw);
    last.replace('_', "-")
}

fn rust_crate_ident(package_name: &str) -> String {
    package_name.replace('-', "_")
}

/// Generate Cargo.toml + src/main.rs for a custom binary (no build).
pub fn generate_custom_project(
    cfg: &CustomGclConfig,
    guff_src: &Path,
    project_dir: &Path,
    config_dir: &Path,
) -> Result<(), CustomError> {
    if cfg.plugins.is_empty() {
        return Err(CustomError::Message(
            "no plugins listed in custom config".into(),
        ));
    }

    fs::create_dir_all(project_dir.join("src"))?;

    let guff_lint_path = guff_src.join("crates/guff-lint");
    let mut deps = String::new();
    deps.push_str(&format!(
        "guff-lint = {{ path = {:?} }}\n",
        guff_lint_path.display()
    ));
    deps.push_str("mimalloc = \"0.1\"\n");

    let mut force_links = String::new();
    for plugin in &cfg.plugins {
        let pkg = cargo_package_name(plugin);
        let ident = rust_crate_ident(&pkg);
        if let Some(path) = &plugin.path {
            let abs = if Path::new(path).is_absolute() {
                PathBuf::from(path)
            } else {
                config_dir.join(path)
            };
            let abs = abs
                .canonicalize()
                .unwrap_or_else(|_| abs);
            deps.push_str(&format!(
                "{pkg} = {{ path = {:?} }}\n",
                abs.display()
            ));
        } else if let Some(version) = &plugin.version {
            let module = &plugin.module;
            if module.contains('/') || module.starts_with("github.com") {
                let git = if module.starts_with("http") {
                    module.clone()
                } else {
                    format!("https://{module}")
                };
                deps.push_str(&format!(
                    "{pkg} = {{ git = {git:?}, tag = {version:?} }}\n"
                ));
            } else {
                deps.push_str(&format!("{pkg} = {version:?}\n"));
            }
        } else {
            return Err(CustomError::Message(format!(
                "plugin {module}: need `path` or `version`",
                module = plugin.module
            )));
        }
        force_links.push_str(&format!("    let _ = {ident}::FORCE_LINK;\n"));
    }

    let cargo_toml = format!(
        r#"[package]
name = "{bin_name}"
version = "0.1.0"
edition = "2021"
publish = false

[[bin]]
name = "{bin_name}"
path = "src/main.rs"

[dependencies]
{deps}
"#,
        bin_name = cfg.name,
        deps = deps,
    );
    fs::write(project_dir.join("Cargo.toml"), cargo_toml)?;

    let main_rs = format!(
        r#"//! Generated by `guff custom`. Do not edit.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> std::process::ExitCode {{
{force_links}
    guff_lint::cli::main()
}}
"#,
        force_links = force_links,
    );
    fs::write(project_dir.join("src/main.rs"), main_rs)?;
    Ok(())
}

/// Options for [`build_custom`].
pub struct BuildCustomOptions {
    pub config: CustomGclConfig,
    /// Directory containing the custom config (for resolving relative plugin paths).
    pub config_dir: PathBuf,
    pub verbose: bool,
    /// Where to place the generated Cargo project (default: temp under destination).
    pub build_dir: Option<PathBuf>,
}

/// Generate, `cargo build --release`, and copy the binary to `destination/name`.
pub fn build_custom(opts: BuildCustomOptions) -> Result<PathBuf, CustomError> {
    let guff_src = resolve_guff_src()?;
    if let Some(ref ver) = opts.config.version {
        let ours = env!("CARGO_PKG_VERSION");
        if ver.trim_start_matches('v') != ours {
            eprintln!(
                "guff: warning: custom config version {ver:?} != guff {ours} (continuing)"
            );
        }
    }

    let dest_dir = PathBuf::from(&opts.config.destination);
    fs::create_dir_all(&dest_dir)?;

    let project_dir = match &opts.build_dir {
        Some(p) => p.clone(),
        None => dest_dir.join(".guff-custom-build"),
    };
    if project_dir.exists() {
        fs::remove_dir_all(&project_dir)?;
    }
    fs::create_dir_all(&project_dir)?;

    generate_custom_project(&opts.config, &guff_src, &project_dir, &opts.config_dir)?;

    if opts.verbose {
        eprintln!("guff: building custom binary in {}", project_dir.display());
    }

    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(project_dir.join("Cargo.toml"));
    if !opts.verbose {
        cmd.arg("--quiet");
    }
    let status = cmd.status()?;
    if !status.success() {
        return Err(CustomError::Cargo(format!(
            "cargo build failed with status {status}"
        )));
    }

    let built = project_dir
        .join("target/release")
        .join(&opts.config.name);
    #[cfg(windows)]
    let built = built.with_extension("exe");

    if !built.is_file() {
        return Err(CustomError::Message(format!(
            "expected binary at {} after cargo build",
            built.display()
        )));
    }

    let out = dest_dir.join(&opts.config.name);
    fs::copy(&built, &out)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&out)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&out, perms)?;
    }

    if opts.verbose {
        eprintln!("guff: wrote {}", out.display());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_golangci_shaped_config() {
        let yaml = r#"
version: "0.1.0"
name: custom-guff
destination: .
plugins:
  - module: guff-plugin-example
    path: ./crates/guff-plugin-example
  - module: github.com/acme/lints
    import: github.com/acme/lints/foo
    version: v1.2.3
"#;
        let cfg = parse_custom_config(yaml).unwrap();
        assert_eq!(cfg.name, "custom-guff");
        assert_eq!(cfg.plugins.len(), 2);
        assert_eq!(cfg.plugins[0].module, "guff-plugin-example");
        assert_eq!(
            cfg.plugins[1].import.as_deref(),
            Some("github.com/acme/lints/foo")
        );
    }

    #[test]
    fn package_name_from_import() {
        let e = CustomPluginEntry {
            module: "github.com/acme/lints".into(),
            import: Some("github.com/acme/lints/foo".into()),
            path: None,
            version: Some("v1".into()),
        };
        assert_eq!(cargo_package_name(&e), "foo");
        assert_eq!(rust_crate_ident("guff-plugin-example"), "guff_plugin_example");
    }

    #[test]
    fn generate_project_smoke() {
        let dir = tempfile::tempdir().unwrap();
        let guff_src = resolve_guff_src().expect("guff src available in tests");
        let cfg = CustomGclConfig {
            version: Some("0.1.0".into()),
            name: "custom-guff".into(),
            destination: ".".into(),
            plugins: vec![CustomPluginEntry {
                module: "guff-plugin-example".into(),
                import: None,
                path: Some(
                    guff_src
                        .join("crates/guff-plugin-example")
                        .to_string_lossy()
                        .into(),
                ),
                version: None,
            }],
        };
        let project = dir.path().join("proj");
        generate_custom_project(&cfg, &guff_src, &project, dir.path()).unwrap();
        let toml = fs::read_to_string(project.join("Cargo.toml")).unwrap();
        assert!(toml.contains("guff-plugin-example"));
        assert!(toml.contains("guff-lint"));
        let main = fs::read_to_string(project.join("src/main.rs")).unwrap();
        assert!(main.contains("guff_lint::cli::main"));
        assert!(main.contains("guff_plugin_example"));
    }
}
