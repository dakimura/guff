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
    let release = goroot_release(&goroot).unwrap_or(VERSION);

    Context {
        goarch: goarch.clone(),
        goos: goos.clone(),
        gopath,
        dir: String::new(),
        cgo_enabled: default_cgo_enabled(&goos, &goarch),
        use_all_files: false,
        compiler: "gc".to_string(),
        build_tags: Vec::new(),
        tool_tags: default_tool_tags(&goarch, &goos, release),
        release_tags: release_tags_for_version(release),
        goroot,
        install_suffix: String::new(),
    }
}

/// The Go 1.N release of the toolchain installed at `goroot`, from the first
/// line of its `VERSION` file (`go1.27.1` → 27).
///
/// Upstream fills `ReleaseTags` from `internal/goversion.Version`, which is
/// compiled into the `go` command — so `go list` (what golangci-lint loads
/// through) answers for the toolchain on PATH, not for any fixed release.
/// guff stands in for that `go` command, so it reads the same toolchain's
/// version. A fixed list hid every `//go:build go1.27` file on a go1.27 host
/// and would admit them on a go1.26 one. `None` (no file, a `devel` build)
/// falls back to [`VERSION`].
pub fn goroot_release(goroot: &str) -> Option<u32> {
    if goroot.is_empty() {
        return None;
    }
    let text = std::fs::read_to_string(Path::new(goroot).join("VERSION")).ok()?;
    let rest = text.lines().next()?.trim().strip_prefix("go1.")?;
    let minor: String = rest.chars().take_while(char::is_ascii_digit).collect();
    minor.parse().ok().filter(|&n| n >= 1)
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

/// Returns `GOROOT` from the environment, PATH layout, well-known install
/// locations, or `go env` as last resort.
///
/// Prefer not spawning `go` (C-3b): after `$GOROOT`, look for a `go` binary on
/// `PATH` and take its toolchain root (Homebrew `…/libexec` included), then
/// probe common install dirs so go-less runs still see stdlib when `go` is
/// absent from a stripped `PATH`. Only if that fails do we shell out to
/// `go env GOROOT` for exotic installers.
fn discover_goroot() -> String {
    let from_env = env_or("GOROOT", "");
    if !from_env.is_empty() {
        return clean_path(&from_env);
    }
    if let Some(root) = goroot_from_path() {
        return clean_path(&root.to_string_lossy());
    }
    for candidate in well_known_goroot_candidates() {
        if candidate.join("VERSION").is_file() && candidate.join("src").is_dir() {
            return clean_path(&candidate.to_string_lossy());
        }
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

/// GOROOT inferred from the `go` binary on PATH (no subprocess).
fn goroot_from_path() -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join("go");
        if !candidate.is_file() {
            continue;
        }
        let real = std::fs::canonicalize(&candidate).unwrap_or(candidate);
        let base = real.parent()?.parent()?;
        for root in [base.to_path_buf(), base.join("libexec")] {
            if root.join("VERSION").is_file() && root.join("src").is_dir() {
                return Some(root);
            }
        }
    }
    None
}

fn well_known_goroot_candidates() -> Vec<PathBuf> {
    let mut out = vec![
        PathBuf::from("/usr/local/go"),
        PathBuf::from("/opt/homebrew/opt/go/libexec"),
        PathBuf::from("/usr/local/opt/go/libexec"),
    ];
    // Homebrew Cellar: /opt/homebrew/Cellar/go/<ver>/libexec
    for cellar in ["/opt/homebrew/Cellar/go", "/usr/local/Cellar/go"] {
        let Ok(entries) = std::fs::read_dir(cellar) else {
            continue;
        };
        let mut versions: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path().join("libexec"))
            .filter(|p| p.join("VERSION").is_file())
            .collect();
        versions.sort();
        out.extend(versions.into_iter().rev()); // newest first
    }
    out
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
/// lines (regabi / greenteagc / jsonv2 / arch versions). Baseline experiments
/// follow the `buildcfg` defaults of the toolchain's release (`release` =
/// N in Go 1.N, as for the release tags); `GOEXPERIMENT` overrides apply on
/// top.
fn default_tool_tags(goarch: &str, goos: &str, release: u32) -> Vec<String> {
    let mut tags = Vec::new();
    match goarch {
        "amd64" => tags.push("amd64.v1".to_string()),
        "386" => tags.push("386.sse2".to_string()),
        "arm64" => tags.push("arm64.v8.0".to_string()),
        _ => {}
    }
    for exp in enabled_goexperiments(goarch, goos, release) {
        tags.push(format!("goexperiment.{exp}"));
    }
    tags
}

/// Experiment names enabled for file matching (`goexperiment.x` tags).
fn enabled_goexperiments(goarch: &str, goos: &str, release: u32) -> Vec<&'static str> {
    parse_goexperiments(
        &env::var("GOEXPERIMENT").unwrap_or_default(),
        goarch,
        goos,
        release,
    )
}

/// The fields of `internal/goexperiment.Flags`, lower-cased, in Go
/// 1.`release` — the names `GOEXPERIMENT` accepts. Flags come and go with
/// releases (1.25 dropped spinbitmutex/coverageredesign, 1.26 dropped
/// synctest/rangefunc/swissmap/…, 1.27 dropped goroutineleakprofile and added
/// mapsplitgroup). Read off each toolchain's `flags.go`.
fn known_goexperiments(release: u32) -> Vec<&'static str> {
    let mut known = vec![
        "fieldtrack",
        "preemptibleloops",
        "staticlockranking",
        "boringcrypto",
        "regabiwrappers",
        "regabiargs",
        "heapminimum512kib",
        "arenas",
        "cgocheck2",
        "loopvar",
        "newinliner",
    ];
    if release <= 25 {
        known.extend([
            "cacheprog",
            "rangefunc",
            "aliastypeparams",
            "swissmap",
            "synchashtriemap",
            "synctest",
        ]);
    }
    if release <= 24 {
        known.extend(["coverageredesign", "spinbitmutex"]);
    }
    if release >= 25 {
        known.extend(["dwarf5", "jsonv2", "greenteagc"]);
    }
    if release >= 26 {
        known.extend([
            "randomizedheapbase64",
            "runtimefreegc",
            "sizespecializedmalloc",
            "simd",
            "runtimesecret",
        ]);
    }
    if release == 26 {
        known.push("goroutineleakprofile");
    }
    if release >= 27 {
        known.push("mapsplitgroup");
    }
    known
}

/// The experiments on for `goarch`/`goos` under Go 1.`release` given a
/// `GOEXPERIMENT` value — `ParseGOEXPERIMENT` of that release's
/// `internal/buildcfg/exp.go`: the baseline, then the comma-separated
/// overrides (`name`, `noname`, `none` anywhere clears everything so far, the
/// `regabi` alias), then regabi forced on where it is always on and off where
/// it is unsupported. An unknown name makes upstream fail outright; it is
/// ignored here.
fn parse_goexperiments(goexp: &str, goarch: &str, goos: &str, release: u32) -> Vec<&'static str> {
    // `regabiSupported` / `regabiAlwaysOn`. s390x moved from supported-only
    // to always-on in 1.27.
    let regabi_always_on = matches!(
        goarch,
        "amd64" | "arm64" | "loong64" | "ppc64le" | "ppc64" | "riscv64"
    ) || (goarch == "s390x" && release >= 27);
    let regabi_supported = regabi_always_on || goarch == "s390x";

    // The `baseline`, which moves with every release: Go 1.27 turned on
    // JSONv2 and SizeSpecializedMalloc, so a go1.27 `go list` builds
    // `encoding/json` from its `v2_*.go` files (importing `encoding/json/v2`,
    // `jsontext`, …) and `internal/goexperiment` from `exp_jsonv2_on.go`. A
    // baseline pinned at 1.26 loaded the v1 files instead —
    // `GUFF_NATIVE_LIST=verify` showed 16 differences on any package
    // importing `encoding/json` under go1.27.1.
    let mut on: Vec<&'static str> = Vec::new();
    if regabi_supported {
        on.extend(["regabiwrappers", "regabiargs"]);
    }
    // `dwarf5Supported` (a flag since 1.25).
    if release >= 25 && !matches!(goos, "darwin" | "ios" | "aix") {
        on.push("dwarf5");
    }
    // Go 1.24 / 1.25 only: these left `Flags` when they became permanent.
    // spinbitmutex is `haveXchg8`, true on every 64-bit arch.
    if release <= 24 {
        on.push("coverageredesign");
        if matches!(
            goarch,
            "amd64" | "arm64" | "loong64" | "ppc64le" | "ppc64" | "riscv64" | "s390x"
        ) {
            on.push("spinbitmutex");
        }
    }
    if release <= 25 {
        on.extend(["aliastypeparams", "swissmap", "synchashtriemap"]);
    }
    if release >= 26 {
        on.extend(["randomizedheapbase64", "greenteagc"]);
    }
    if release >= 27 {
        on.extend(["jsonv2", "sizespecializedmalloc"]);
    }

    let known = known_goexperiments(release);
    for part in goexp.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part == "none" {
            on.clear();
            continue;
        }
        let (name, enable) = match part.strip_prefix("no") {
            Some(name) => (name, false),
            None => (part, true),
        };
        let names: &[&str] = if name == "regabi" {
            &["regabiwrappers", "regabiargs"]
        } else {
            &[name]
        };
        // `GOEXPERIMENT=jsonv2` used to be dropped on the floor here (the
        // list stopped at ten names), and `encoding/json/v2` / `jsontext` —
        // whose files are all `//go:build goexperiment.jsonv2` on 1.26 —
        // loaded empty: trivy lints with that experiment.
        for n in names {
            let Some(k) = known.iter().copied().find(|k| k == n) else {
                continue;
            };
            on.retain(|e| *e != k);
            if enable {
                on.push(k);
            }
        }
    }

    for k in ["regabiwrappers", "regabiargs"] {
        on.retain(|e| *e != k);
        if regabi_always_on {
            on.push(k);
        }
    }
    on
}

#[cfg(test)]
mod goexperiment_tests {
    use super::{default_tool_tags, parse_goexperiments};

    /// The release reaches the tool tags `match_file` reads: a go1.27 GOROOT
    /// selects `encoding/json`'s `//go:build goexperiment.jsonv2` files.
    #[test]
    fn tool_tags_carry_the_release_baseline() {
        if std::env::var_os("GOEXPERIMENT").is_some() {
            return;
        }
        let t27 = default_tool_tags("arm64", "darwin", 27);
        assert!(t27.iter().any(|t| t == "goexperiment.jsonv2"), "{t27:?}");
        assert!(t27.iter().any(|t| t == "arm64.v8.0"), "{t27:?}");
        let t26 = default_tool_tags("arm64", "darwin", 26);
        assert!(!t26.iter().any(|t| t == "goexperiment.jsonv2"), "{t26:?}");
    }

    #[test]
    fn jsonv2_is_a_known_experiment() {
        let on = parse_goexperiments("jsonv2", "arm64", "darwin", 26);
        assert!(on.contains(&"jsonv2"), "{on:?}");
        // The baseline stays on underneath an addition.
        assert!(on.contains(&"greenteagc"), "{on:?}");
    }

    #[test]
    fn overrides_add_remove_and_reset() {
        let on = parse_goexperiments("jsonv2,nogreenteagc,arenas", "arm64", "linux", 26);
        assert!(on.contains(&"jsonv2") && on.contains(&"arenas"), "{on:?}");
        assert!(!on.contains(&"greenteagc"), "{on:?}");
        assert!(on.contains(&"dwarf5"), "dwarf5 is on for linux: {on:?}");
        // `none` clears the baseline, but regabi is forced back on where it
        // is always on (`regabiAlwaysOn`).
        assert_eq!(
            parse_goexperiments("none", "arm64", "linux", 27),
            ["regabiwrappers", "regabiargs"]
        );
        assert!(parse_goexperiments("none", "386", "linux", 27).is_empty());
        // An unknown name is ignored, as buildcfg rejects it.
        assert!(!parse_goexperiments("nosuchexperiment", "arm64", "linux", 27).contains(&"nosuchexperiment"));
    }

    /// The baseline follows the toolchain's `buildcfg`: go1.27.1 turns on
    /// jsonv2 and sizespecializedmalloc, go1.26 does not (both read from
    /// GOROOT/src/internal/buildcfg/exp.go of those toolchains).
    #[test]
    fn baseline_follows_the_release() {
        let mut g126 = parse_goexperiments("", "arm64", "linux", 26);
        g126.sort_unstable();
        assert_eq!(
            g126,
            ["dwarf5", "greenteagc", "randomizedheapbase64", "regabiargs", "regabiwrappers"]
        );
        let mut g127 = parse_goexperiments("", "arm64", "darwin", 27);
        g127.sort_unstable();
        assert_eq!(
            g127,
            [
                "greenteagc",
                "jsonv2",
                "randomizedheapbase64",
                "regabiargs",
                "regabiwrappers",
                "sizespecializedmalloc"
            ]
        );
        // An override can still turn a baseline experiment off.
        assert!(!parse_goexperiments("nojsonv2", "arm64", "linux", 27).contains(&"jsonv2"));
        let g125 = parse_goexperiments("", "arm64", "linux", 25);
        assert!(g125.contains(&"swissmap") && !g125.contains(&"greenteagc"), "{g125:?}");
    }

    /// `ParseGOEXPERIMENT`'s edge cases: `none` mid-list, the regabi alias
    /// and its arch overrides, and names that exist only in some releases.
    #[test]
    fn parse_follows_upstream_edge_cases() {
        // `none` anywhere resets what came before it.
        let on = parse_goexperiments("arenas,none,jsonv2", "386", "linux", 27);
        assert_eq!(on, ["jsonv2"]);
        // `noregabi` is undone on an always-on arch, and regabi is never on
        // where it is unsupported.
        let on = parse_goexperiments("noregabi", "amd64", "linux", 27);
        assert!(on.contains(&"regabiargs") && on.contains(&"regabiwrappers"), "{on:?}");
        let on = parse_goexperiments("regabi", "386", "linux", 27);
        assert!(!on.contains(&"regabiargs"), "{on:?}");
        // s390x: supported-only before 1.27, always on from 1.27.
        assert!(!parse_goexperiments("noregabi", "s390x", "linux", 26).contains(&"regabiargs"));
        assert!(parse_goexperiments("noregabi", "s390x", "linux", 27).contains(&"regabiargs"));
        // synctest is a 1.24/1.25 flag; goroutineleakprofile is 1.26 only.
        assert!(parse_goexperiments("synctest", "arm64", "linux", 24).contains(&"synctest"));
        assert!(!parse_goexperiments("synctest", "arm64", "linux", 26).contains(&"synctest"));
        let leak = "goroutineleakprofile";
        assert!(parse_goexperiments(leak, "arm64", "linux", 26).contains(&leak));
        assert!(!parse_goexperiments(leak, "arm64", "linux", 27).contains(&leak));
        assert!(parse_goexperiments("mapsplitgroup", "arm64", "linux", 27).contains(&"mapsplitgroup"));
        // Go 1.24 baseline: no dwarf5 flag yet, spinbitmutex on 64-bit.
        let mut g124 = parse_goexperiments("", "amd64", "linux", 24);
        g124.sort_unstable();
        assert_eq!(
            g124,
            [
                "aliastypeparams",
                "coverageredesign",
                "regabiargs",
                "regabiwrappers",
                "spinbitmutex",
                "swissmap",
                "synchashtriemap"
            ]
        );
    }
}
