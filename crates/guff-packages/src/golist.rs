//! `go list -json` driver.
//!
//! Port of `golist.go` (`goListDriver`, `createDriverResponse`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde::Deserialize;

use crate::config::Config;
use crate::load_mode::LoadMode;
use crate::package::{DriverResponse, Error, ErrorKind, Module, ModuleError, Package};
use crate::typecheck::TypecheckEnv;

/// Errors from the `go list` driver.
#[derive(Debug)]
pub enum GoListError {
    GoNotFound(String),
    CommandFailed { status: String, stderr: String },
    Json(String),
    List(Error),
    Internal(String),
}

impl std::fmt::Display for GoListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GoNotFound(msg) => write!(f, "'go list' driver requires 'go': {msg}"),
            Self::CommandFailed { status, stderr } => {
                write!(f, "go list failed ({status}): {stderr}")
            }
            Self::Json(msg) => write!(f, "JSON decoding failed: {msg}"),
            Self::List(err) => write!(f, "{err}"),
            Self::Internal(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for GoListError {}

impl From<GoListError> for crate::LoadError {
    fn from(value: GoListError) -> Self {
        Self::Driver(value.to_string())
    }
}

/// Loads packages by invoking `go list -json`.
///
/// Warm runs reuse a disk cache of the stdout under `$GUFF_CACHE/golist/`
/// (R24.4) when the go.mod/sum fingerprint, args, and env are unchanged and
/// every cached `Export` path still exists on disk.
pub fn go_list_driver(cfg: &Config, patterns: &[String]) -> Result<DriverResponse, GoListError> {
    let timing = std::env::var_os("GUFF_DEBUG_CACHE").is_some();
    let t_invoke = std::time::Instant::now();
    let mode = cfg.effective_mode();
    let args = golist_args(cfg, patterns, 0);
    let stdout = load_or_invoke_go(cfg, patterns, &args)?;
    if timing {
        eprintln!(
            "guff:   golist invoke(main) {:.2}s ({} bytes)",
            t_invoke.elapsed().as_secs_f64(),
            stdout.len(),
        );
    }
    let t_parse = std::time::Instant::now();

    let mut response = DriverResponse::default();
    let env = cfg.resolved_env();
    response.compiler = "gc".to_string();
    response.arch = TypecheckEnv::from_env(&env, "gc").arch;
    let mut seen: HashMap<String, JsonPackage> = HashMap::new();
    let mut additional_errors: HashMap<String, Vec<Error>> = HashMap::new();

    let stream = serde_json::Deserializer::from_str(&stdout).into_iter::<JsonPackage>();
    for item in stream {
        let p = item.map_err(|e| GoListError::Json(e.to_string()))?;
        if p.import_path.is_empty() {
            if let Some(err) = p.error {
                return Err(GoListError::List(Error {
                    pos: err.pos,
                    msg: err.err,
                    kind: ErrorKind::List,
                }));
            }
            return Err(GoListError::Internal(format!(
                "package missing import path: {p:?}"
            )));
        }

        if let Some(old) = seen.get(&p.import_path) {
            if old.error.is_none() && p.error.is_none() {
                if old != &p {
                    return Err(GoListError::Internal(format!(
                        "go list gives conflicting information for package {}",
                        p.import_path
                    )));
                }
                continue;
            }
            if old.error.is_some() && p.error.is_none() {
                continue;
            }
        }
        seen.insert(p.import_path.clone(), p.clone());

        let pkg = json_package_to_package(&p, cfg)?;
        if !p.dep_only {
            response.roots.push(pkg.id.clone());
        }
        response.packages.push(Arc::new(pkg));
    }

    for (id, pkg) in response
        .packages
        .iter_mut()
        .map(|p| (p.id.clone(), Arc::make_mut(p)))
    {
        if let Some(extra) = additional_errors.remove(&id) {
            pkg.errors.extend(extra);
        }
    }

    if timing {
        eprintln!(
            "guff:   golist parse+build {:.2}s ({} pkgs)",
            t_parse.elapsed().as_secs_f64(),
            response.packages.len(),
        );
    }
    let t_stdlib = std::time::Instant::now();

    // Hybrid source mode: the main call ran without `-export` (so third-party
    // deps are not compiled). Type info for those comes from source, but stdlib
    // is still resolved from export data — build only the stdlib `.a` here (a
    // small, ~4s-cold subset) via a second, stdlib-restricted `go list -export`.
    //
    // Optional warm-GOCACHE reuse: when the build cache already holds third-party
    // `.a` files, a further `go list -export` returns those paths cheaply and
    // `build_source_seed` prefers them over source typecheck (large seed win).
    // Gated by [`export_reuse_enabled`] so a cold empty GOCACHE stays source-only.
    if cfg.dep_source {
        let stdlib: Vec<String> = seen
            .values()
            .filter(|p| p.standard && p.import_path != "unsafe" && p.error.is_none())
            .map(|p| p.import_path.clone())
            .collect();
        if !stdlib.is_empty() {
            let exports = load_or_fetch_stdlib_exports(cfg, &stdlib, timing)?;
            for pkg in response.packages.iter_mut() {
                if let Some(export) = exports.get(&pkg.id) {
                    if !export.is_empty() {
                        Arc::make_mut(pkg).export_file = PathBuf::from(export);
                    }
                }
            }
        }

        let third_party: Vec<String> = seen
            .values()
            .filter(|p| {
                !p.standard
                    && p.error.is_none()
                    && p.import_path != "unsafe"
                    // Test-variant ids look like `pkg [pkg.test]` — not valid
                    // `go list` arguments and would abort the whole run.
                    && !p.import_path.contains(' ')
                    && !p.import_path.ends_with(".test")
                    // Only external modules: main-module packages would force
                    // `go list -export` to recompile local sources (net loss).
                    && p.module.as_ref().is_some_and(|m| !m.main)
            })
            .map(|p| p.import_path.clone())
            .collect();
        if !third_party.is_empty() {
            let mode = export_reuse_mode();
            if mode != ExportReuseMode::Off {
                let t_reuse = std::time::Instant::now();
                let exports = match mode {
                    ExportReuseMode::Off => HashMap::new(),
                    ExportReuseMode::CachedOnly => load_dep_export_cache(cfg, &third_party),
                    ExportReuseMode::Fetch => {
                        match fetch_package_exports(cfg, &third_party) {
                            Ok(map) => {
                                store_dep_export_cache(cfg, &map);
                                map
                            }
                            Err(e) => {
                                if timing {
                                    eprintln!(
                                        "guff:   golist dep-export-reuse failed after {:.2}s ({e}); using source seed",
                                        t_reuse.elapsed().as_secs_f64(),
                                    );
                                }
                                HashMap::new()
                            }
                        }
                    }
                };
                if !exports.is_empty() {
                    let mut attached = 0usize;
                    for pkg in response.packages.iter_mut() {
                        if let Some(export) = exports.get(&pkg.id) {
                            if !export.is_empty() && Path::new(export).exists() {
                                Arc::make_mut(pkg).export_file = PathBuf::from(export);
                                attached += 1;
                            }
                        }
                    }
                    if timing {
                        eprintln!(
                            "guff:   golist dep-export-reuse {:.2}s ({} attached / {} candidates, mode={:?})",
                            t_reuse.elapsed().as_secs_f64(),
                            attached,
                            third_party.len(),
                            mode,
                        );
                    }
                } else if timing && mode == ExportReuseMode::CachedOnly {
                    eprintln!(
                        "guff:   golist dep-export-reuse cache miss ({} candidates); source seed",
                        third_party.len(),
                    );
                }
            }
        }
    }

    if timing {
        eprintln!(
            "guff:   golist stdlib-export {:.2}s",
            t_stdlib.elapsed().as_secs_f64(),
        );
    }

    let _ = mode; // mode informs golist_args; refine clears fields later.
    Ok(response)
}

/// Stdlib export paths, from disk cache when possible (PERF_TASKS_V2 B-8).
///
/// The stdlib `go list -export` is the *dominant* cost of a warm run: measured
/// on prometheus `./...`, warm `load_graph` is 0.22s and this subprocess is
/// 0.15s of it — 42% of the whole 0.36s warm wall. The main `go list` stdout is
/// already cached (`load_or_invoke_go`), and re-parsing its 14MB of JSON costs
/// only 0.03s, so this second subprocess is what warm runs actually wait on.
///
/// The answer is a pure function of the toolchain and the requested package
/// set, both of which are in the key, so caching the `import_path → .a` map
/// removes the subprocess entirely on a warm run.
fn load_or_fetch_stdlib_exports(
    cfg: &Config,
    paths: &[String],
    timing: bool,
) -> Result<HashMap<String, String>, GoListError> {
    if !golist_cache_enabled(cfg) {
        return fetch_package_exports(cfg, paths);
    }
    let cache_path = stdlib_export_cache_path(cfg, paths);
    if let Some(p) = cache_path.as_ref() {
        if let Some(map) = load_stdlib_export_cache(p) {
            if timing {
                eprintln!(
                    "guff:   golist stdlib-export cache hit ({} pkgs)",
                    map.len(),
                );
            }
            return Ok(map);
        }
    }
    let map = fetch_package_exports(cfg, paths)?;
    if let Some(p) = cache_path.as_ref() {
        store_stdlib_export_cache(p, &map);
    }
    Ok(map)
}

/// Bump when the on-disk shape of the stdlib export cache changes; an old file
/// then simply misses instead of being misread (`PERF_TASKS.md` Task 4 lesson).
const STDLIB_EXPORT_CACHE_VERSION: &str = "stdlib-export-v1";

fn stdlib_export_cache_path(cfg: &Config, paths: &[String]) -> Option<PathBuf> {
    let dir = guff_cache_dir()?;
    // Feed the version, the toolchain identity and the exact requested set
    // through `golist_cache_key`, which already folds in dir/tests/mode, build
    // flags, go.mod+go.sum and the `go list`-relevant env subset.
    let mut args = Vec::with_capacity(paths.len() + 2);
    args.push(STDLIB_EXPORT_CACHE_VERSION.to_string());
    args.push(go_toolchain_fingerprint());
    let mut sorted = paths.to_vec();
    sorted.sort();
    args.extend(sorted);
    let key = golist_cache_key(cfg, &[], &args);
    let prefix = key.get(..2).unwrap_or("00");
    Some(
        dir.join("stdlib_exports")
            .join(prefix)
            .join(format!("{key}.json")),
    )
}

/// Identity of the `go` toolchain that produced the cached export paths.
///
/// "The cached `.a` still exists" is not sufficient on its own: the archives
/// live in GOCACHE, which survives a toolchain upgrade, so after upgrading Go
/// the previous stdlib archives can still be on disk and we would type-check
/// against them. `golist_cache_key`'s env subset only covers this when GOROOT /
/// GOVERSION happen to be exported, which they usually are not. So resolve the
/// `go` binary on `PATH` (matching [`invoke_go`], which spawns plain `go`) and
/// fingerprint it. A few `stat` calls; asking `go env GOVERSION` would cost a
/// subprocess, which is the very thing this cache exists to avoid.
fn go_toolchain_fingerprint() -> String {
    let Some(path) = std::env::var_os("PATH") else {
        return "go=unknown".to_string();
    };
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("go");
        // `metadata` follows symlinks, so version-manager shims report the
        // real toolchain rather than the (stable) link.
        let Ok(meta) = std::fs::metadata(&candidate) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        let real = std::fs::canonicalize(&candidate).unwrap_or(candidate);
        return format!("go={} len={} mtime={}", real.display(), meta.len(), mtime);
    }
    "go=unknown".to_string()
}

/// Load a cached stdlib export map, or `None` when it is missing, unreadable,
/// corrupt, or references an archive that is no longer on disk (GOCACHE is
/// cleaned independently of `GUFF_CACHE`). Every failure falls back to a fresh
/// `go list -export` — a bad cache file must never abort the run.
fn load_stdlib_export_cache(path: &Path) -> Option<HashMap<String, String>> {
    let bytes = std::fs::read(path).ok()?;
    let map: HashMap<String, String> = serde_json::from_slice(&bytes).ok()?;
    if map.is_empty() {
        return None;
    }
    if map
        .values()
        .any(|v| v.is_empty() || !Path::new(v).exists())
    {
        return None;
    }
    Some(map)
}

fn store_stdlib_export_cache(path: &Path, map: &HashMap<String, String>) {
    if map.is_empty() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(bytes) = serde_json::to_vec(map) else {
        return;
    };
    // tmp + rename so a concurrent reader never sees a half-written file.
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Second `go list -export` for hybrid source mode: resolve export `.a` paths
/// for the given packages (stdlib and/or warm-GOCACHE third-party reuse).
///
/// Large candidate sets are batched to stay under `ARG_MAX`.
fn fetch_package_exports(
    cfg: &Config,
    paths: &[String],
) -> Result<HashMap<String, String>, GoListError> {
    if paths.is_empty() {
        return Ok(HashMap::new());
    }
    const BATCH: usize = 200;
    let mut map = HashMap::new();
    for chunk in paths.chunks(BATCH) {
        let mut args = vec![
            "list".to_string(),
            "-json=ImportPath,Export".to_string(),
            "-export".to_string(),
        ];
        args.extend(chunk.iter().cloned());
        let stdout = invoke_go(cfg, &args)?;
        let stream = serde_json::Deserializer::from_str(&stdout).into_iter::<JsonExport>();
        for item in stream {
            let e = item.map_err(|e| GoListError::Json(e.to_string()))?;
            if !e.export.is_empty() {
                map.insert(e.import_path, e.export);
            }
        }
    }
    Ok(map)
}

/// How hybrid mode may attach warm-GOCACHE third-party export `.a` files.
///
/// - unset / `0|false|off` → [`Off`] (default). Pure hybrid source seed.
/// - `auto` → [`CachedOnly`]: reuse a prior path map from
///   `~/.cache/guff/dep_exports/` when `.a` files still exist. Never invokes
///   `go list -export`. Measured slower+fatter on prometheus (export-decoded
///   SSA); leave opt-in until that cost is fixed.
/// - `1|true|on|fetch` → [`Fetch`]: run `go list -export` and refresh the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportReuseMode {
    Off,
    CachedOnly,
    Fetch,
}

fn export_reuse_mode() -> ExportReuseMode {
    match std::env::var("GUFF_EXPORT_REUSE") {
        Err(_) => ExportReuseMode::Off,
        Ok(v) => {
            let v = v.to_ascii_lowercase();
            if matches!(v.as_str(), "0" | "false" | "off" | "no") {
                ExportReuseMode::Off
            } else if matches!(v.as_str(), "1" | "true" | "on" | "yes" | "fetch") {
                ExportReuseMode::Fetch
            } else if v == "auto" {
                ExportReuseMode::CachedOnly
            } else {
                ExportReuseMode::Off
            }
        }
    }
}

fn dep_export_cache_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("guff").join("dep_exports"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        return std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join("Library/Caches/guff/dep_exports")
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/guff/dep_exports"))
    }
}

fn dep_export_cache_path(cfg: &Config) -> Option<PathBuf> {
    let dir = dep_export_cache_dir()?;
    let key = golist_cache_key(
        cfg,
        &[],
        &["dep-export-reuse-v1".into()],
    );
    Some(dir.join(format!("{key}.json")))
}

/// Load a previously stored import_path → Export map, keeping only entries
/// whose files still exist. Returns empty when the cache is missing/stale.
fn load_dep_export_cache(cfg: &Config, want: &[String]) -> HashMap<String, String> {
    let Some(path) = dep_export_cache_path(cfg) else {
        return HashMap::new();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return HashMap::new();
    };
    let Ok(stored) = serde_json::from_slice::<HashMap<String, String>>(&bytes) else {
        return HashMap::new();
    };
    let want: std::collections::HashSet<&str> = want.iter().map(String::as_str).collect();
    let mut out = HashMap::new();
    for (k, v) in stored {
        if want.contains(k.as_str()) && !v.is_empty() && Path::new(&v).exists() {
            out.insert(k, v);
        }
    }
    // Require a useful fraction so a nearly-empty stale cache doesn't attach
    // a handful of deps and leave the rest on a mixed (riskier) seed path.
    if out.len() * 10 < want.len() * 8 {
        return HashMap::new();
    }
    out
}

fn store_dep_export_cache(cfg: &Config, map: &HashMap<String, String>) {
    let Some(path) = dep_export_cache_path(cfg) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec(map) {
        let _ = std::fs::write(path, bytes);
    }
}

fn load_or_invoke_go(
    cfg: &Config,
    patterns: &[String],
    args: &[String],
) -> Result<String, GoListError> {
    if let Some(cached) = try_load_golist_cache(cfg, patterns, args) {
        if export_paths_exist(&cached) {
            return Ok(cached);
        }
    }
    let stdout = invoke_go(cfg, args)?;
    store_golist_cache(cfg, patterns, args, &stdout);
    Ok(stdout)
}

const GOLIST_CACHE_VERSION: &str = "golist-v1";

fn golist_cache_enabled(cfg: &Config) -> bool {
    if cfg.disable_cache {
        return false;
    }
    for key in ["GUFF_CACHE", "GOLANGCI_LINT_CACHE"] {
        if let Ok(v) = std::env::var(key) {
            if v == "off" {
                return false;
            }
        }
    }
    true
}

fn guff_cache_dir() -> Option<PathBuf> {
    for key in ["GUFF_CACHE", "GOLANGCI_LINT_CACHE"] {
        if let Ok(v) = std::env::var(key) {
            if v.is_empty() || v == "off" {
                continue;
            }
            let p = PathBuf::from(&v);
            if p.is_absolute() {
                return Some(p);
            }
        }
    }
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("guff"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        return std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Caches/guff"));
    }
    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("LOCALAPPDATA").map(|h| PathBuf::from(h).join("guff"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/guff"));
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

fn golist_cache_key(cfg: &Config, patterns: &[String], args: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(GOLIST_CACHE_VERSION.as_bytes());
    h.update(b"\n");
    h.update(format!("dir={}\n", cfg.dir.display()).as_bytes());
    h.update(format!("tests={}\n", cfg.tests).as_bytes());
    h.update(format!("mode={:?}\n", cfg.effective_mode()).as_bytes());

    let mut flags = cfg.build_flags.clone();
    flags.sort();
    for f in &flags {
        h.update(format!("flag={f}\n").as_bytes());
    }

    let mut pats = patterns.to_vec();
    if pats.is_empty() {
        pats.push(".".into());
    }
    pats.sort();
    for p in &pats {
        h.update(format!("pat={p}\n").as_bytes());
    }

    for a in args {
        h.update(format!("arg={a}\n").as_bytes());
    }

    // Fingerprint go.mod / go.sum near cfg.dir (walk up a few levels).
    if let Some(mod_dir) = find_go_mod_dir(&cfg.dir) {
        for name in ["go.mod", "go.sum"] {
            let path = mod_dir.join(name);
            match std::fs::read(&path) {
                Ok(bytes) => {
                    h.update(format!("file={name}\n").as_bytes());
                    h.update(&bytes);
                    h.update(b"\n");
                }
                Err(_) => {
                    h.update(format!("file={name}=missing\n").as_bytes());
                }
            }
        }
    }

    // Env subset that affects `go list` / export paths.
    let env = cfg.resolved_env();
    let mut interesting: Vec<(String, String)> = Vec::new();
    for entry in &env {
        if let Some((k, v)) = entry.split_once('=') {
            if matches!(
                k,
                "GOOS" | "GOARCH" | "CGO_ENABLED" | "GOTOOLCHAIN" | "GOROOT" | "GOFLAGS" | "GOVERSION"
            ) {
                interesting.push((k.to_string(), v.to_string()));
            }
        }
    }
    interesting.sort();
    for (k, v) in interesting {
        h.update(format!("env {k}={v}\n").as_bytes());
    }

    hex_encode(&h.finalize())
}

fn find_go_mod_dir(start: &Path) -> Option<PathBuf> {
    let mut cur = if start.as_os_str().is_empty() {
        std::env::current_dir().ok()?
    } else {
        start.to_path_buf()
    };
    for _ in 0..32 {
        if cur.join("go.mod").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            break;
        }
    }
    None
}

fn golist_cache_path(key: &str) -> Option<PathBuf> {
    let dir = guff_cache_dir()?;
    let prefix = key.get(..2).unwrap_or("00");
    Some(dir.join("golist").join(prefix).join(format!("{key}.json")))
}

fn try_load_golist_cache(cfg: &Config, patterns: &[String], args: &[String]) -> Option<String> {
    if !golist_cache_enabled(cfg) {
        return None;
    }
    let key = golist_cache_key(cfg, patterns, args);
    let path = golist_cache_path(&key)?;
    std::fs::read_to_string(path).ok()
}

fn store_golist_cache(cfg: &Config, patterns: &[String], args: &[String], stdout: &str) {
    if !golist_cache_enabled(cfg) {
        return;
    }
    let key = golist_cache_key(cfg, patterns, args);
    let Some(path) = golist_cache_path(&key) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, stdout).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Returns false when any non-empty Export path in the cached stdout is missing
/// (GOCACHE may have been cleaned independently of GUFF_CACHE).
fn export_paths_exist(stdout: &str) -> bool {
    let stream = serde_json::Deserializer::from_str(stdout).into_iter::<JsonPackage>();
    for item in stream {
        let Ok(p) = item else {
            return false;
        };
        if !p.export.is_empty() && !Path::new(&p.export).exists() {
            return false;
        }
    }
    true
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn json_package_to_package(p: &JsonPackage, cfg: &Config) -> Result<Package, GoListError> {
    let dir = PathBuf::from(&p.dir);
    let mut pkg = Package {
        name: p.name.clone(),
        id: p.import_path.clone(),
        dir: dir.clone(),
        target: path_from_maybe_relative(&p.target, &dir),
        go_files: abs_join(&dir, &merge_slices(&p.go_files, &p.cgo_files)),
        compiled_go_files: filter_compiled_go_files(abs_join(
            &dir,
            &p.compiled_go_files,
        )),
        other_files: abs_join(&dir, &other_files(p)),
        embed_files: abs_join(&dir, &p.embed_files),
        embed_patterns: abs_join(&dir, &p.embed_patterns),
        ignored_files: abs_join(
            &dir,
            &merge_slices(&p.ignored_go_files, &p.ignored_other_files),
        ),
        for_test: p.for_test.clone(),
        deps: p.deps.clone(),
        module: p.module.as_ref().map(json_module_to_module),
        ..Package::default()
    };

    if p.export.is_empty() {
        pkg.export_file = PathBuf::new();
    } else if Path::new(&p.export).is_absolute() {
        pkg.export_file = PathBuf::from(&p.export);
    } else {
        pkg.export_file = dir.join(&p.export);
    }

    if let Some(space) = pkg.id.find(' ') {
        pkg.pkg_path = pkg.id[..space].to_string();
    } else {
        pkg.pkg_path = pkg.id.clone();
    }

    if pkg.pkg_path == "unsafe" {
        pkg.compiled_go_files.clear();
    } else if pkg.compiled_go_files.is_empty() {
        pkg.compiled_go_files = pkg.go_files.clone();
    }

    if let Some(err) = &p.error {
        pkg.errors.push(Error {
            pos: err.pos.clone(),
            msg: err.err.clone(),
            kind: ErrorKind::List,
        });
    }

    pkg.imports = build_import_stubs(&p.imports, &p.import_map);

    let _ = cfg;
    Ok(pkg)
}

fn json_module_to_module(m: &JsonModule) -> Module {
    Module {
        path: m.path.clone(),
        version: m.version.clone().unwrap_or_default(),
        replace: m.replace.as_ref().map(|r| Box::new(json_module_to_module(r))),
        main: m.main,
        indirect: m.indirect,
        dir: PathBuf::from(m.dir.as_deref().unwrap_or_default()),
        go_mod: PathBuf::from(m.go_mod.as_deref().unwrap_or_default()),
        go_version: m.go_version.clone().unwrap_or_default(),
        error: m.error.as_ref().map(|e| ModuleError {
            err: e.err.clone(),
        }),
    }
}

fn build_import_stubs(imports: &[String], import_map: &HashMap<String, String>) -> HashMap<String, Arc<Package>> {
    let mut ids: HashMap<String, bool> = imports.iter().map(|id| (id.clone(), true)).collect();
    let mut out = HashMap::new();
    for (path, id) in import_map {
        out.insert(
            path.clone(),
            Arc::new(Package {
                id: id.clone(),
                ..Package::default()
            }),
        );
        ids.remove(id);
    }
    for id in ids.keys() {
        if id == "C" {
            continue;
        }
        out.insert(
            id.clone(),
            Arc::new(Package {
                id: id.clone(),
                ..Package::default()
            }),
        );
    }
    out
}

fn golist_args(cfg: &Config, patterns: &[String], go_version: u32) -> Vec<String> {
    let mode = cfg.effective_mode();
    const FIND_FLAGS: LoadMode = LoadMode(
        LoadMode::NEED_IMPORTS.0
            | LoadMode::NEED_TYPES.0
            | LoadMode::NEED_SYNTAX.0
            | LoadMode::NEED_TYPES_INFO.0,
    );

    let mut args = vec![
        "list".to_string(),
        "-e".to_string(),
        json_flag(cfg, go_version),
        format!(
            "-compiled={}",
            mode.contains(LoadMode::NEED_COMPILED_GO_FILES)
                || mode.contains(LoadMode::NEED_SYNTAX)
                || mode.contains(LoadMode::NEED_TYPES)
                || mode.contains(LoadMode::NEED_TYPES_INFO)
                || mode.contains(LoadMode::NEED_TYPES_SIZES)
        ),
        format!("-test={}", cfg.tests),
        format!("-export={}", uses_export_data(cfg)),
        format!("-deps={}", mode.contains(LoadMode::NEED_IMPORTS)),
        format!(
            "-find={}",
            !cfg.tests && (mode & FIND_FLAGS) == LoadMode::empty() && !uses_export_data(cfg)
        ),
    ];

    if go_version >= 21 {
        args.push("-pgo=off".to_string());
    }

    args.extend(cfg.build_flags.clone());
    args.push("--".to_string());
    if patterns.is_empty() {
        args.push(".".to_string());
    } else {
        args.extend(patterns.iter().cloned());
    }
    args
}

fn json_flag(cfg: &Config, go_version: u32) -> String {
    if go_version < 19 {
        return "-json".to_string();
    }

    let mode = cfg.effective_mode();
    let mut fields = Vec::new();
    let mut added = HashMap::<String, bool>::new();
    let mut add = |list: &[&str]| {
        for f in list {
            if !added.contains_key(*f) {
                added.insert((*f).to_string(), true);
                fields.push((*f).to_string());
            }
        }
    };

    add(&["Name", "ImportPath", "Error"]);
    if cfg.dep_source {
        // Needed to classify stdlib (resolved via export data) vs third-party
        // (type-checked from source) in the hybrid source mode.
        add(&["Standard"]);
    }
    if mode.contains(LoadMode::NEED_FILES)
        || mode.contains(LoadMode::NEED_TYPES)
        || mode.contains(LoadMode::NEED_TYPES_INFO)
    {
        add(&[
            "Dir",
            "GoFiles",
            "IgnoredGoFiles",
            "IgnoredOtherFiles",
            "CFiles",
            "CgoFiles",
            "CXXFiles",
            "MFiles",
            "HFiles",
            "FFiles",
            "SFiles",
            "SwigFiles",
            "SwigCXXFiles",
            "SysoFiles",
        ]);
        if cfg.tests {
            add(&["TestGoFiles", "XTestGoFiles"]);
        }
    }
    if mode.contains(LoadMode::NEED_TYPES) || mode.contains(LoadMode::NEED_TYPES_INFO) {
        add(&["Dir", "CompiledGoFiles"]);
    }
    if mode.contains(LoadMode::NEED_COMPILED_GO_FILES) {
        add(&["Dir", "CompiledGoFiles", "Export"]);
    }
    if mode.contains(LoadMode::NEED_IMPORTS) {
        add(&["DepOnly", "Imports", "ImportMap"]);
        if cfg.tests {
            add(&["TestImports", "XTestImports"]);
        }
    }
    if mode.contains(LoadMode::NEED_DEPS) {
        add(&["DepOnly", "Deps"]);
    }
    if uses_export_data(cfg) {
        add(&["Dir", "Export"]);
    }
    if mode.contains(LoadMode::NEED_FOR_TEST) {
        add(&["ForTest"]);
    }
    if mode.contains(LoadMode::NEED_MODULE) {
        add(&["Module"]);
    }
    if mode.contains(LoadMode::NEED_EMBED_FILES) {
        add(&["EmbedFiles"]);
    }
    if mode.contains(LoadMode::NEED_EMBED_PATTERNS) {
        add(&["EmbedPatterns"]);
    }
    if mode.contains(LoadMode::NEED_TARGET) {
        add(&["Target"]);
    }

    format!("-json={}", fields.join(","))
}

fn uses_export_data(cfg: &Config) -> bool {
    // Source mode resolves dependency types by type-checking source, so `go list`
    // must not build export data (`-export`) — that build is the cold-path cost
    // this mode exists to avoid.
    if cfg.dep_source {
        return false;
    }
    let mode = cfg.effective_mode();
    mode.contains(LoadMode::NEED_EXPORT_FILE)
        || (mode.contains(LoadMode::NEED_TYPES) && !mode.contains(LoadMode::NEED_DEPS))
}

fn invoke_go(cfg: &Config, args: &[String]) -> Result<String, GoListError> {
    let mut cmd = Command::new("go");
    if !cfg.dir.as_os_str().is_empty() {
        cmd.current_dir(&cfg.dir);
    }
    cmd.args(args);
    let mut env = parse_env(&cfg.resolved_env());
    ensure_gocache(&mut env);
    cmd.envs(env);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GoListError::GoNotFound(e.to_string())
        } else {
            GoListError::GoNotFound(e.to_string())
        }
    })?;

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(GoListError::CommandFailed {
            status: output.status.to_string(),
            stderr,
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Ensure `GOCACHE` is set for `go list` so export/cgo artifacts land in a
/// known directory (PL07). Prefer an existing env value; otherwise use the
/// same default Go would pick (`$XDG_CACHE_HOME/go-build` / platform cache).
fn ensure_gocache(env: &mut Vec<(String, String)>) {
    if env.iter().any(|(k, _)| k == "GOCACHE") {
        return;
    }
    if let Ok(v) = std::env::var("GOCACHE") {
        if !v.is_empty() {
            env.push(("GOCACHE".into(), v));
            return;
        }
    }
    if let Some(dir) = default_gocache_path() {
        env.push(("GOCACHE".into(), dir.to_string_lossy().into_owned()));
    }
}

fn default_gocache_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("go-build"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        return std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Caches/go-build"));
    }
    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("LOCALAPPDATA").map(|h| PathBuf::from(h).join("go-build"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/go-build"));
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

fn parse_env(env: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in env {
        if let Some((k, v)) = entry.split_once('=') {
            out.push((k.to_string(), v.to_string()));
        }
    }
    out
}

fn abs_join(dir: &Path, files: &[String]) -> Vec<PathBuf> {
    files
        .iter()
        .map(|f| {
            let p = Path::new(f);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                dir.join(p)
            }
        })
        .collect()
}

fn path_from_maybe_relative(path: &str, dir: &Path) -> PathBuf {
    if path.is_empty() {
        return PathBuf::new();
    }
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        dir.join(p)
    }
}

fn filter_compiled_go_files(files: Vec<PathBuf>) -> Vec<PathBuf> {
    files
        .into_iter()
        .filter(|f| {
            match f.extension().and_then(|s| s.to_str()) {
                Some("go") => true,
                None => true, // cgo-processed cache file
                Some(_) => false,
            }
        })
        .collect()
}

fn merge_slices(a: &[String], b: &[String]) -> Vec<String> {
    let mut out = a.to_vec();
    out.extend_from_slice(b);
    out
}

fn other_files(p: &JsonPackage) -> Vec<String> {
    let mut out = Vec::new();
    for slice in [
        &p.c_files,
        &p.cxx_files,
        &p.m_files,
        &p.h_files,
        &p.f_files,
        &p.s_files,
        &p.swig_files,
        &p.swig_cxx_files,
        &p.syso_files,
    ] {
        out.extend_from_slice(slice);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct JsonPackage {
    #[serde(rename = "ImportPath")]
    import_path: String,
    #[serde(default, rename = "Dir")]
    dir: String,
    #[serde(default, rename = "Name")]
    name: String,
    #[serde(default, rename = "Target")]
    target: String,
    #[serde(default, rename = "Export")]
    export: String,
    #[serde(default, rename = "GoFiles")]
    go_files: Vec<String>,
    #[serde(default, rename = "CompiledGoFiles")]
    compiled_go_files: Vec<String>,
    #[serde(default, rename = "IgnoredGoFiles")]
    ignored_go_files: Vec<String>,
    #[serde(default, rename = "IgnoredOtherFiles")]
    ignored_other_files: Vec<String>,
    #[serde(default, rename = "EmbedPatterns")]
    embed_patterns: Vec<String>,
    #[serde(default, rename = "EmbedFiles")]
    embed_files: Vec<String>,
    #[serde(default, rename = "CFiles")]
    c_files: Vec<String>,
    #[serde(default, rename = "CgoFiles")]
    cgo_files: Vec<String>,
    #[serde(default, rename = "CXXFiles")]
    cxx_files: Vec<String>,
    #[serde(default, rename = "MFiles")]
    m_files: Vec<String>,
    #[serde(default, rename = "HFiles")]
    h_files: Vec<String>,
    #[serde(default, rename = "FFiles")]
    f_files: Vec<String>,
    #[serde(default, rename = "SFiles")]
    s_files: Vec<String>,
    #[serde(default, rename = "SwigFiles")]
    swig_files: Vec<String>,
    #[serde(default, rename = "SwigCXXFiles")]
    swig_cxx_files: Vec<String>,
    #[serde(default, rename = "SysoFiles")]
    syso_files: Vec<String>,
    #[serde(default, rename = "Imports")]
    imports: Vec<String>,
    #[serde(default, rename = "ImportMap")]
    import_map: HashMap<String, String>,
    #[serde(default, rename = "Deps")]
    deps: Vec<String>,
    #[serde(default, rename = "Module")]
    module: Option<JsonModule>,
    #[serde(default, rename = "ForTest")]
    for_test: String,
    #[serde(default, rename = "DepOnly")]
    dep_only: bool,
    #[serde(default, rename = "Standard")]
    standard: bool,
    #[serde(default, rename = "Error")]
    error: Option<JsonPackageError>,
}

/// Minimal shape for the second `go list -export` call used by hybrid source
/// mode ([`Config::dep_source`]) for stdlib and warm-GOCACHE third-party reuse.
#[derive(Debug, Clone, Deserialize)]
struct JsonExport {
    #[serde(rename = "ImportPath")]
    import_path: String,
    #[serde(default, rename = "Export")]
    export: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct JsonModule {
    #[serde(rename = "Path")]
    path: String,
    #[serde(default, rename = "Version")]
    version: Option<String>,
    #[serde(default, rename = "Replace")]
    replace: Option<Box<JsonModule>>,
    #[serde(default, rename = "Main")]
    main: bool,
    #[serde(default, rename = "Indirect")]
    indirect: bool,
    #[serde(default, rename = "Dir")]
    dir: Option<String>,
    #[serde(default, rename = "GoMod")]
    go_mod: Option<String>,
    #[serde(default, rename = "GoVersion")]
    go_version: Option<String>,
    #[serde(default, rename = "Error")]
    error: Option<JsonModuleError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct JsonModuleError {
    #[serde(rename = "Err")]
    err: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct JsonPackageError {
    #[serde(default, rename = "ImportStack")]
    import_stack: Vec<String>,
    #[serde(default, rename = "Pos")]
    pos: String,
    #[serde(rename = "Err")]
    err: String,
}

/// Returns true when `go` is available on PATH.
pub fn go_available() -> bool {
    Command::new("go")
        .arg("version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Canonicalize a local pattern the way golangci-lint does.
pub fn normalize_pattern(pattern: &str) -> String {
    let p = Path::new(pattern);
    if pattern.starts_with('.') || p.has_root() || starts_with_drive_letter(pattern) {
        pattern.to_string()
    } else {
        format!(".{}{pattern}", std::path::MAIN_SEPARATOR)
    }
}

fn starts_with_drive_letter(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_pattern_adds_dot_slash_for_bare_name() {
        assert_eq!(
            normalize_pattern("foo"),
            format!(".{}foo", std::path::MAIN_SEPARATOR)
        );
    }

    #[test]
    fn normalize_pattern_keeps_dot_relative() {
        assert_eq!(normalize_pattern("./foo"), "./foo");
    }

    #[test]
    fn json_flag_includes_imports_when_needed() {
        let cfg = Config {
            mode: LoadMode::NEED_IMPORTS,
            ..Config::default()
        };
        let flag = json_flag(&cfg, 22);
        assert!(flag.contains("Imports"));
        assert!(flag.contains("ImportMap"));
    }

    #[test]
    fn uses_export_data_for_types_without_deps() {
        let cfg = Config {
            mode: LoadMode::NEED_TYPES,
            dep_source: false,
            ..Config::default()
        };
        assert!(uses_export_data(&cfg));
    }

    #[test]
    fn dep_source_skips_export_data() {
        let cfg = Config {
            mode: LoadMode::NEED_TYPES | LoadMode::NEED_EXPORT_FILE,
            dep_source: true,
            ..Config::default()
        };
        assert!(!uses_export_data(&cfg));
    }

    #[test]
    fn golist_cache_key_stable_and_sensitive() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/golist");
        let cfg = Config {
            mode: LoadMode::LOAD_IMPORTS,
            dir: dir.clone(),
            disable_cache: false,
            ..Config::default()
        };
        let pats = vec![".".to_string()];
        let args = golist_args(&cfg, &pats, 0);
        let k1 = golist_cache_key(&cfg, &pats, &args);
        let k2 = golist_cache_key(&cfg, &pats, &args);
        assert_eq!(k1, k2);
        assert_eq!(k1.len(), 64);

        let cfg2 = Config {
            tests: true,
            ..cfg.clone()
        };
        let args2 = golist_args(&cfg2, &pats, 0);
        let k3 = golist_cache_key(&cfg2, &pats, &args2);
        assert_ne!(k1, k3);

        let cfg3 = Config {
            disable_cache: true,
            ..cfg
        };
        assert!(!golist_cache_enabled(&cfg3));
    }

    #[test]
    fn stdlib_export_cache_roundtrips_and_rejects_bad_files() {
        let tmp = std::env::temp_dir().join(format!(
            "guff-stdlib-export-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        std::fs::create_dir_all(&tmp).expect("mkdir");
        let path = tmp.join("exports.json");

        // A real archive path must exist on disk for the load to be accepted,
        // so point at a file we create ourselves.
        let archive = tmp.join("fmt.a");
        std::fs::write(&archive, b"not really an archive").expect("write archive");
        let mut map = HashMap::new();
        map.insert("fmt".to_string(), archive.display().to_string());

        store_stdlib_export_cache(&path, &map);
        assert_eq!(load_stdlib_export_cache(&path).as_ref(), Some(&map));

        // Archive deleted out from under us (GOCACHE cleaned) → miss, not a
        // stale hit against a vanished path.
        std::fs::remove_file(&archive).expect("rm archive");
        assert!(load_stdlib_export_cache(&path).is_none());

        // Corrupt / truncated / empty content → miss, never a panic.
        std::fs::write(&path, b"{\"fmt\": \"/tmp/tr").expect("write truncated");
        assert!(load_stdlib_export_cache(&path).is_none());
        std::fs::write(&path, b"not json at all").expect("write garbage");
        assert!(load_stdlib_export_cache(&path).is_none());
        std::fs::write(&path, b"{}").expect("write empty map");
        assert!(load_stdlib_export_cache(&path).is_none());

        // Missing file → miss.
        std::fs::remove_file(&path).expect("rm cache");
        assert!(load_stdlib_export_cache(&path).is_none());

        // An empty map is not worth a file.
        store_stdlib_export_cache(&path, &HashMap::new());
        assert!(!path.exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn stdlib_export_cache_key_tracks_requested_set_and_toolchain() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/golist");
        let cfg = Config {
            mode: LoadMode::LOAD_IMPORTS,
            dir,
            disable_cache: false,
            ..Config::default()
        };
        let a = stdlib_export_cache_path(&cfg, &["fmt".into(), "io".into()]);
        let b = stdlib_export_cache_path(&cfg, &["io".into(), "fmt".into()]);
        let c = stdlib_export_cache_path(&cfg, &["fmt".into()]);
        // Order must not matter (the set is sorted into the key) …
        assert_eq!(a, b);
        // … but the membership must.
        assert_ne!(a, c);
        if let Some(p) = a {
            assert!(p.to_string_lossy().contains("stdlib_exports"));
        }

        // The fingerprint is stable within a run; either it found `go` on PATH
        // or it says so explicitly — never an empty string that would silently
        // key every toolchain the same.
        let fp = go_toolchain_fingerprint();
        assert_eq!(fp, go_toolchain_fingerprint());
        assert!(fp.starts_with("go="), "unexpected fingerprint: {fp}");
    }

    #[test]
    fn export_paths_exist_rejects_missing() {
        let stdout = r#"{"ImportPath":"x","Export":"/nonexistent/guff-export-missing.a"}"#;
        assert!(!export_paths_exist(stdout));
        let stdout_ok = r#"{"ImportPath":"x","Export":""}"#;
        assert!(export_paths_exist(stdout_ok));
    }
}
