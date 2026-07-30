//! [`Context`] — the supporting context for a build.
//!
//! Port of `go/build.Context` and `defaultContext` from `build.go`.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use guff_goversion::VERSION;

/// A [`Context`] specifies the supporting context for a build.
///
/// Equivalent to `build.Context`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    /// Target architecture (`GOARCH`).
    pub goarch: String,
    /// Target operating system (`GOOS`).
    pub goos: String,
    /// Go root (`GOROOT`).
    pub goroot: String,
    /// Go paths (`GOPATH`).
    pub gopath: String,

    /// Caller's working directory, or empty to use the process cwd.
    /// In module mode this locates the main module.
    pub dir: String,

    /// Whether cgo files are included.
    pub cgo_enabled: bool,
    /// Use files regardless of `go:build` lines and file names.
    pub use_all_files: bool,
    /// Compiler to assume when computing target paths.
    pub compiler: String,

    /// Build constraints satisfied when processing `go:build` lines.
    /// Defaults to empty for [`DEFAULT`]; clients may customize.
    pub build_tags: Vec<String>,
    /// Toolchain build tags (normally do not customize).
    pub tool_tags: Vec<String>,
    /// Go release tags the current release is compatible with.
    /// The last element is the current release.
    pub release_tags: Vec<String>,

    /// Suffix for installation directory names (e.g. `"race"`).
    pub install_suffix: String,
}

/// The default [`Context`] for builds.
///
/// Equivalent to `build.Default`.
pub static DEFAULT: std::sync::LazyLock<Context> = std::sync::LazyLock::new(default_context);

impl Default for Context {
    fn default() -> Self {
        default_context()
    }
}

impl Context {
    /// Returns a copy of this context with additional build tags appended.
    pub fn with_build_tags<I, S>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for tag in tags {
            self.build_tags.push(tag.as_ref().to_string());
        }
        self
    }
}

/// Constructs the default build context from the environment.
///
/// Equivalent to `build.defaultContext`.
pub fn default_context() -> Context {
    let goarch = env_or("GOARCH", host_goarch());
    let goos = env_or("GOOS", host_goos());
    let goroot = discover_goroot();
    let gopath = env_or("GOPATH", &default_gopath(&goroot));

    Context {
        goarch: goarch.clone(),
        goos: goos.clone(),
        goroot,
        gopath,
        dir: String::new(),
        cgo_enabled: default_cgo_enabled(&goos, &goarch),
        use_all_files: false,
        compiler: "gc".to_string(),
        build_tags: Vec::new(),
        tool_tags: default_tool_tags(&goarch),
        release_tags: release_tags_for_version(VERSION),
        install_suffix: String::new(),
    }
}

/// Builds the list of Go release tags `go1.1` … `go1.N` for Go 1.N.
///
/// Equivalent to the loop in `build.defaultContext` that fills `ReleaseTags`.
pub fn release_tags_for_version(version: u32) -> Vec<String> {
    (1..=version).map(|i| format!("go1.{i}")).collect()
}

fn env_or(name: &str, default: &str) -> String {
    match env::var(name) {
        Ok(value) if !value.is_empty() => value,
        _ => default.to_string(),
    }
}

/// Maps the host Rust `std::env::consts::ARCH` to a Go `GOARCH` value.
fn host_goarch() -> &'static str {
    match env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        "arm" => "arm",
        "riscv64" => "riscv64",
        "powerpc64" => "ppc64",
        "powerpc64le" => "ppc64le",
        "s390x" => "s390x",
        "wasm32" => "wasm",
        other => other,
    }
}

/// Maps the host Rust `std::env::consts::OS` to a Go `GOOS` value.
fn host_goos() -> &'static str {
    match env::consts::OS {
        "macos" => "darwin",
        other => other,
    }
}

/// Returns `GOROOT` from the environment, or from `go env GOROOT` if available.
fn discover_goroot() -> String {
    let from_env = env_or("GOROOT", "");
    if !from_env.is_empty() {
        return clean_path(&from_env);
    }
    if let Ok(output) = Command::new("go").args(["env", "GOROOT"]).output() {
        if output.status.success() {
            let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !root.is_empty() {
                return clean_path(&root);
            }
        }
    }
    String::new()
}

/// Default `GOPATH` when the environment variable is unset.
///
/// Equivalent to `build.defaultGOPATH`.
fn default_gopath(goroot: &str) -> String {
    let home_env = match host_goos() {
        "windows" => "USERPROFILE",
        "plan9" => "home",
        _ => "HOME",
    };
    let Some(home) = env::var(home_env).ok().filter(|s| !s.is_empty()) else {
        return String::new();
    };
    let def = Path::new(&home).join("go");
    let def = clean_path(&def.to_string_lossy());
    if !goroot.is_empty() && def == clean_path(goroot) {
        // Don't set default GOPATH to GOROOT (common misconfiguration).
        return String::new();
    }
    def
}

fn clean_path(path: &str) -> String {
    // Go uses filepath.Clean; for our purposes normalize separators only.
    PathBuf::from(path).to_string_lossy().into_owned()
}

/// Default `CGO_ENABLED` when the environment variable is unset.
///
/// Simplified port of `build.defaultContext`'s cgo branch: enabled on native
/// builds for common OS/arch pairs, disabled for cross-compilation.
fn default_cgo_enabled(target_goos: &str, target_goarch: &str) -> bool {
    match env::var("CGO_ENABLED").ok().as_deref() {
        Some("1") => return true,
        Some("0") => return false,
        Some(_) => {}
        None => {}
    }
    let host_goos = host_goos();
    let host_goarch = host_goarch();
    if host_goos != target_goos || host_goarch != target_goarch {
        return false;
    }
    cgo_supported(target_goos, target_goarch)
}

/// Reports whether cgo is supported on `goos`/`goarch`.
///
/// Minimal port of `internal/platform.CgoSupported` for common targets.
fn cgo_supported(goos: &str, goarch: &str) -> bool {
    matches!(
        (goos, goarch),
        ("darwin", "amd64" | "arm64")
            | ("linux", "386" | "amd64" | "arm" | "arm64" | "ppc64le" | "riscv64" | "s390x")
            | ("freebsd", "386" | "amd64" | "arm")
            | ("openbsd", "386" | "amd64" | "arm" | "arm64")
            | ("windows", "amd64" | "arm" | "arm64")
    )
}

/// Toolchain tags for the target architecture and enabled GOEXPERIMENTs.
///
/// Mirrors `internal/buildcfg.toolTags` closely enough for stdlib `go:build`
/// lines (regabi / greenteagc / arch versions). Baseline experiments follow
/// Go 1.26's `buildcfg` defaults; `GOEXPERIMENT` overrides apply on top.
fn default_tool_tags(goarch: &str) -> Vec<String> {
    let mut tags = Vec::new();
    match goarch {
        "amd64" => tags.push("amd64.v1".to_string()),
        "386" => tags.push("386.sse2".to_string()),
        "arm64" => tags.push("arm64.v8.0".to_string()),
        _ => {}
    }
    for exp in enabled_goexperiments() {
        tags.push(format!("goexperiment.{exp}"));
    }
    tags
}

/// Experiment names enabled for file matching (`goexperiment.x` tags).
fn enabled_goexperiments() -> Vec<&'static str> {
    // Baseline for Go ≥ 1.26 (see GOROOT/src/internal/buildcfg/exp.go).
    // Older toolchains ignore unknown experiment tags in match_file.
    let mut on = vec![
        "regabiwrappers",
        "regabiargs",
        "randomizedheapbase64",
        "greenteagc",
    ];
    // Dwarf5 is on except darwin/ios/aix — we approximate with host OS.
    if !matches!(host_goos(), "darwin" | "ios" | "aix") {
        on.push("dwarf5");
    }

    let goexp = env::var("GOEXPERIMENT").unwrap_or_default();
    if goexp.is_empty() {
        return on;
    }
    if goexp == "none" {
        return Vec::new();
    }
    for part in goexp.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(name) = part.strip_prefix("no") {
            if name == "regabi" {
                on.retain(|e| *e != "regabiwrappers" && *e != "regabiargs");
            } else {
                on.retain(|e| *e != name);
            }
            continue;
        }
        if part == "regabi" {
            for e in ["regabiwrappers", "regabiargs"] {
                if !on.contains(&e) {
                    on.push(e);
                }
            }
            continue;
        }
        // Only accept names we know about (static lifetime).
        let known = [
            "regabiwrappers",
            "regabiargs",
            "randomizedheapbase64",
            "greenteagc",
            "dwarf5",
            "fieldtrack",
            "boringcrypto",
            "staticlockranking",
            "heapminimum512kib",
            "preemptibleloops",
        ];
        if let Some(k) = known.iter().copied().find(|k| *k == part) {
            if !on.contains(&k) {
                on.push(k);
            }
        }
    }
    on
}
