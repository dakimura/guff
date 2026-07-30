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
/// (R24.4) when the go.mod/sum fingerprint, **the set of `.go` file names under
/// the root patterns**, args, and env are unchanged and every cached `Export`
/// path still exists on disk. File *contents* are intentionally not hashed
/// here — the issue cache already keys on content; this fingerprint only has
/// to catch package add/remove (X-1).
pub fn go_list_driver(cfg: &Config, patterns: &[String]) -> Result<DriverResponse, GoListError> {
    let timing = crate::debug::enabled();
    let t_invoke = std::time::Instant::now();
    let mode = cfg.effective_mode();
    let args = golist_args(cfg, patterns, go_minor_version(cfg));
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
        // C-3a: the main call ran with `-compiled=false`, so the cgo/SWIG
        // packages still need their real `CompiledGoFiles`. That query and the
        // stdlib export query are independent subprocesses, and each is mostly
        // the ~0.078s `go` startup, so run them together rather than back to
        // back.
        let want_compiled = if defers_compiled(cfg) {
            needs_compiled_query(&seen)
        } else {
            Vec::new()
        };

        let (exports, compiled) = std::thread::scope(|s| {
            let compiled_job = (!want_compiled.is_empty())
                .then(|| s.spawn(|| load_or_fetch_compiled_files(cfg, &want_compiled, timing)));
            let exports = if stdlib.is_empty() {
                Ok(HashMap::new())
            } else {
                load_or_fetch_stdlib_exports(cfg, &stdlib, timing)
            };
            let compiled = compiled_job.map_or(Ok(HashMap::new()), |j| {
                j.join().unwrap_or_else(|_| {
                    Err(GoListError::Internal("compiled-files query panicked".into()))
                })
            });
            (exports, compiled)
        });

        let exports = exports?;
        for pkg in response.packages.iter_mut() {
            if let Some(export) = exports.get(&pkg.id) {
                if !export.is_empty() {
                    Arc::make_mut(pkg).export_file = PathBuf::from(export);
                }
            }
        }

        // A failed cgo query must not abort the run: the packages keep the
        // `GoFiles` fallback, exactly as before this call existed.
        match compiled {
            Ok(map) => {
                let attached = attach_compiled_files(&mut response.packages, &map);
                if timing && !want_compiled.is_empty() {
                    eprintln!(
                        "guff:   golist compiled-files {attached} attached / {} cgo pkgs",
                        want_compiled.len(),
                    );
                }
            }
            Err(e) if timing => {
                eprintln!("guff:   golist compiled-files failed ({e}); using GoFiles");
            }
            Err(_) => {}
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
    let key = golist_cache_key(cfg, &[], &args, false);
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

/// Go minor version of the toolchain that will serve `go list` (26 for
/// go1.26.4), or 0 when it cannot be determined.
///
/// This gates `-json=<fields>` (Go 1.19+) and `-pgo=off` (Go 1.21+), so it runs
/// on every load. `go env GOVERSION` would answer it exactly — and costs a
/// 0.074s subprocess, which is the same order as the flag it is deciding about.
/// `$GOROOT/VERSION` is a one-line file (measured 0.011ms) holding the same
/// string, so read that instead. Returning 0 is always safe: the caller falls
/// back to the pre-1.19 flag set.
fn go_minor_version(cfg: &Config) -> u32 {
    static CACHE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    // The env override is per-Config, so only the PATH-derived answer is worth
    // memoizing; an explicit GOROOT is a cheap path join.
    if let Some(root) = env_goroot(cfg) {
        if let Some(v) = parse_goroot_version(&root) {
            return v;
        }
    }
    *CACHE.get_or_init(|| {
        go_root_from_path()
            .as_deref()
            .and_then(parse_goroot_version)
            .unwrap_or(0)
    })
}

fn env_goroot(cfg: &Config) -> Option<PathBuf> {
    for entry in cfg.resolved_env() {
        if let Some(v) = entry.strip_prefix("GOROOT=") {
            if !v.is_empty() {
                return Some(PathBuf::from(v));
            }
        }
    }
    None
}

/// GOROOT inferred from the `go` binary on PATH, without running it.
///
/// After canonicalization `…/bin/go`'s grandparent is GOROOT for both the
/// upstream layout (`/usr/local/go/bin/go`) and Homebrew's
/// (`…/Cellar/go/1.26.4/libexec/bin/go`). The `libexec` child is tried too for
/// packagings that do not resolve the symlink into it. A candidate only counts
/// when it holds both `VERSION` and `src/`, so a wrong guess degrades to 0
/// rather than to a wrong version.
fn go_root_from_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("go");
        if !candidate.is_file() {
            continue;
        }
        let real = std::fs::canonicalize(&candidate).unwrap_or(candidate);
        let Some(base) = real.parent().and_then(Path::parent) else {
            continue;
        };
        for root in [base.to_path_buf(), base.join("libexec")] {
            if root.join("VERSION").is_file() && root.join("src").is_dir() {
                return Some(root);
            }
        }
    }
    None
}

/// Minor version from a `$GOROOT/VERSION` first line such as `go1.26.4`.
fn parse_goroot_version(root: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(root.join("VERSION")).ok()?;
    let line = text.lines().next()?.trim();
    let rest = line.strip_prefix("go")?;
    let minor = rest.split('.').nth(1)?;
    // Trim any pre-release suffix (`1.21rc1` → `21`).
    let digits: String = minor.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
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

/// Bump when the on-disk shape of the compiled-files cache changes.
const COMPILED_FILES_CACHE_VERSION: &str = "compiled-files-v1";

fn compiled_files_cache_path(cfg: &Config, paths: &[String]) -> Option<PathBuf> {
    let dir = guff_cache_dir()?;
    let mut args = Vec::with_capacity(paths.len() + 2);
    args.push(COMPILED_FILES_CACHE_VERSION.to_string());
    args.push(go_toolchain_fingerprint());
    let mut sorted = paths.to_vec();
    sorted.sort();
    args.extend(sorted);
    let key = golist_cache_key(cfg, &[], &args, false);
    let prefix = key.get(..2).unwrap_or("00");
    Some(
        dir.join("compiled_files")
            .join(prefix)
            .join(format!("{key}.json")),
    )
}

/// `import_path → CompiledGoFiles` for the cgo/SWIG packages, from disk when
/// possible (a warm run must not pay a subprocess the main call no longer pays).
///
/// Mirrors [`load_or_fetch_stdlib_exports`]: cgo-generated files live in
/// GOCACHE, which is cleaned independently of `GUFF_CACHE`, so a cached entry
/// naming a file that is gone must miss rather than resurrect a dead path.
fn load_compiled_files_cache(path: &Path) -> Option<HashMap<String, Vec<String>>> {
    let bytes = std::fs::read(path).ok()?;
    let map: HashMap<String, Vec<String>> = serde_json::from_slice(&bytes).ok()?;
    if map.is_empty() {
        return None;
    }
    if map
        .values()
        .any(|files| files.is_empty() || files.iter().any(|f| !Path::new(f).exists()))
    {
        return None;
    }
    Some(map)
}

fn store_compiled_files_cache(path: &Path, map: &HashMap<String, Vec<String>>) {
    if map.is_empty() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(bytes) = serde_json::to_vec(map) else {
        return;
    };
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn load_or_fetch_compiled_files(
    cfg: &Config,
    paths: &[String],
    timing: bool,
) -> Result<HashMap<String, Vec<String>>, GoListError> {
    if !golist_cache_enabled(cfg) {
        return fetch_compiled_files(cfg, paths);
    }
    let cache_path = compiled_files_cache_path(cfg, paths);
    if let Some(p) = cache_path.as_ref() {
        if let Some(map) = load_compiled_files_cache(p) {
            if timing {
                eprintln!(
                    "guff:   golist compiled-files cache hit ({} pkgs)",
                    map.len(),
                );
            }
            return Ok(map);
        }
    }
    let map = fetch_compiled_files(cfg, paths)?;
    if let Some(p) = cache_path.as_ref() {
        store_compiled_files_cache(p, &map);
    }
    Ok(map)
}

/// Read-only compiled-files map from disk (no subprocess). `None` on miss.
fn peek_compiled_files_cache(
    cfg: &Config,
    paths: &[String],
) -> Option<HashMap<String, Vec<String>>> {
    for key in ["GUFF_CACHE", "GOLANGCI_LINT_CACHE"] {
        if let Ok(v) = std::env::var(key) {
            if v == "off" {
                return None;
            }
        }
    }
    let path = compiled_files_cache_path(cfg, paths)?;
    load_compiled_files_cache(&path)
}

/// Second `go list -compiled=true`, restricted to the cgo/SWIG packages.
///
/// `CompiledGoFiles` mixes two kinds of path: the package's own sources come
/// back as bare file names relative to `Dir`, while the cgo-generated ones are
/// absolute GOCACHE paths. `Dir` is requested so both can be stored absolute —
/// the cache validates entries by testing that every file still exists, and a
/// bare name would fail that test from any working directory.
fn fetch_compiled_files(
    cfg: &Config,
    paths: &[String],
) -> Result<HashMap<String, Vec<String>>, GoListError> {
    if paths.is_empty() {
        return Ok(HashMap::new());
    }
    const BATCH: usize = 200;
    let mut map = HashMap::new();
    for chunk in paths.chunks(BATCH) {
        let mut args = vec![
            "list".to_string(),
            "-e".to_string(),
            "-json=ImportPath,Dir,CompiledGoFiles".to_string(),
            "-compiled=true".to_string(),
        ];
        args.extend(cfg.build_flags.clone());
        args.push("--".to_string());
        args.extend(chunk.iter().cloned());
        let stdout = invoke_go(cfg, &args)?;
        let stream = serde_json::Deserializer::from_str(&stdout).into_iter::<JsonCompiled>();
        for item in stream {
            let c = item.map_err(|e| GoListError::Json(e.to_string()))?;
            if c.compiled_go_files.is_empty() {
                continue;
            }
            let files = abs_join(Path::new(&c.dir), &c.compiled_go_files)
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            map.insert(c.import_path, files);
        }
    }
    Ok(map)
}

/// Overwrite `compiled_go_files` for the packages the second call answered for.
fn attach_compiled_files(
    packages: &mut [Arc<Package>],
    compiled: &HashMap<String, Vec<String>>,
) -> usize {
    if compiled.is_empty() {
        return 0;
    }
    let mut attached = 0;
    for pkg in packages.iter_mut() {
        let Some(files) = compiled.get(&pkg.id) else {
            continue;
        };
        Arc::make_mut(pkg).compiled_go_files =
            filter_compiled_go_files(files.iter().map(PathBuf::from).collect());
        attached += 1;
    }
    attached
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
        false,
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
    // `detail` splits the level-1 `golist invoke(main)` total into the parts that
    // behave differently: the disk-cache probe (warm) and the `go list`
    // subprocess (cold, and the one C-3 would have to replace).
    let detail = crate::debug::detailed();
    let t_probe = std::time::Instant::now();
    if let Some(cached) = try_load_golist_cache(cfg, patterns, args) {
        if export_paths_exist(&cached) {
            if detail {
                eprintln!(
                    "guff:     golist cache probe {:.2}s (hit, {} bytes)",
                    t_probe.elapsed().as_secs_f64(),
                    cached.len(),
                );
            }
            return Ok(cached);
        }
    }
    if detail {
        eprintln!(
            "guff:     golist cache probe {:.2}s (miss)",
            t_probe.elapsed().as_secs_f64(),
        );
    }
    let t_exec = std::time::Instant::now();
    let stdout = invoke_go(cfg, args)?;
    let exec = t_exec.elapsed();
    let t_store = std::time::Instant::now();
    store_golist_cache(cfg, patterns, args, &stdout);
    if detail {
        eprintln!(
            "guff:     golist subprocess {:.2}s ({} bytes), cache store {:.2}s",
            exec.as_secs_f64(),
            stdout.len(),
            t_store.elapsed().as_secs_f64(),
        );
    }
    Ok(stdout)
}

/// Bump when the key composition changes so stale entries under the old hash
/// space are never looked up (they just rot and get cleaned eventually).
const GOLIST_CACHE_VERSION: &str = "golist-v2";

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

/// Build the golist / stdlib-export / dep-export cache key.
///
/// When `include_go_files` is true (main `go list` stdout cache only), the
/// sorted set of `.go` **file names** under the root patterns is folded in so
/// that adding or deleting a package directory invalidates the cache (X-1).
/// File contents are not hashed — content edits are the issue cache's job.
/// Stdlib/dep-export callers pass `false` so local package churn does not
/// needlessly bust their keys.
fn golist_cache_key(
    cfg: &Config,
    patterns: &[String],
    args: &[String],
    include_go_files: bool,
) -> String {
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
    let mod_dir = find_go_mod_dir(&cfg.dir);
    if let Some(mod_dir) = mod_dir.as_ref() {
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

    if include_go_files {
        let base = if cfg.dir.as_os_str().is_empty() {
            std::env::current_dir().unwrap_or_default()
        } else {
            cfg.dir.clone()
        };
        let module_path = mod_dir
            .as_ref()
            .and_then(|d| read_module_path(&d.join("go.mod")));
        hash_go_file_set(
            &mut h,
            &base,
            mod_dir.as_deref(),
            &pats,
            module_path.as_deref(),
        );
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

/// Fold the sorted set of relative `.go` paths under `patterns` into `h`.
///
/// Cost on prometheus `./...` is ~4–6 ms (725 files) — fine next to warm
/// `load_graph` of ~0.07 s. Names only; see [`golist_cache_key`].
fn hash_go_file_set(
    h: &mut impl sha2::Digest,
    base: &Path,
    module_root: Option<&Path>,
    patterns: &[String],
    module_path: Option<&str>,
) {
    let mut files = Vec::new();
    for pat in patterns {
        collect_go_files_for_pattern(base, module_root, pat, module_path, &mut files);
    }
    files.sort();
    files.dedup();
    h.update(format!("go_files={}\n", files.len()).as_bytes());
    for f in &files {
        h.update(f.as_bytes());
        h.update(b"\n");
    }
}

/// Collect `.go` file paths (relative to `base`, `/`-separated) matching a
/// single `go list` pattern. Local (`./…`, `.`, absolute) and current-module
/// path patterns are walked; other patterns (std, other modules) are skipped
/// — those trees are not under `base` and go.mod already covers them.
fn collect_go_files_for_pattern(
    base: &Path,
    module_root: Option<&Path>,
    pattern: &str,
    module_path: Option<&str>,
    out: &mut Vec<String>,
) {
    let (root, recursive) = match resolve_pattern_root(base, module_root, pattern, module_path) {
        Some(v) => v,
        None => {
            // Record the unresolved pattern so a later resolution (e.g. after
            // `go get`) still changes the key rather than silently sticking.
            out.push(format!("unresolved:{pattern}"));
            return;
        }
    };
    if !root.exists() {
        out.push(format!("missing:{}", root.display()));
        return;
    }
    if recursive {
        walk_go_files(base, &root, out);
    } else {
        collect_go_files_in_dir(base, &root, out);
    }
}

fn resolve_pattern_root(
    base: &Path,
    module_root: Option<&Path>,
    pattern: &str,
    module_path: Option<&str>,
) -> Option<(PathBuf, bool)> {
    let (prefix, recursive) = if pattern == "..." {
        (".", true)
    } else if let Some(p) = pattern.strip_suffix("/...") {
        (if p.is_empty() { "." } else { p }, true)
    } else {
        (pattern, false)
    };

    let p = Path::new(prefix);
    if prefix == "." || prefix.is_empty() {
        return Some((base.to_path_buf(), recursive));
    }
    if prefix.starts_with('.') || p.is_absolute() {
        let root = if p.is_absolute() {
            p.to_path_buf()
        } else {
            base.join(p)
        };
        return Some((root, recursive));
    }
    // `example.com/mod/...` → filesystem under the go.mod directory.
    if let (Some(mod_path), Some(mod_root)) = (module_path, module_root) {
        if prefix == mod_path {
            return Some((mod_root.to_path_buf(), recursive));
        }
        if let Some(rel) = prefix.strip_prefix(mod_path) {
            let rel = rel.trim_start_matches('/');
            let root = if rel.is_empty() {
                mod_root.to_path_buf()
            } else {
                mod_root.join(rel)
            };
            return Some((root, recursive));
        }
    }
    None
}

fn walk_go_files(base: &Path, root: &Path, out: &mut Vec<String>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Some(name) = dir.file_name().and_then(|s| s.to_str()) {
            // Match `go list ./...` skips (also mirrored in offline::walk_packages).
            if dir != root
                && (name == "vendor"
                    || name == "testdata"
                    || name == "node_modules"
                    || name.starts_with('.')
                    || name.starts_with('_'))
            {
                continue;
            }
        }
        collect_go_files_in_dir(base, &dir, out);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
}

fn collect_go_files_in_dir(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.ends_with(".go") {
            continue;
        }
        let rel = path.strip_prefix(base).unwrap_or(&path);
        out.push(rel.to_string_lossy().replace('\\', "/"));
    }
}

/// Best-effort `module` path from a go.mod (first `module …` line). Used only
/// to map `example.com/mod/...` patterns onto the filesystem under `base`.
fn read_module_path(go_mod: &Path) -> Option<String> {
    let text = std::fs::read_to_string(go_mod).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("module ") {
            let path = rest.trim().trim_matches('"');
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
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
    let key = golist_cache_key(cfg, patterns, args, true);
    let path = golist_cache_path(&key)?;
    std::fs::read_to_string(path).ok()
}

/// Like [`try_load_golist_cache`], but ignores `cfg.disable_cache`.
///
/// Used by C-7 speculation: `--no-cache` still wants to *read* a previous warm
/// run's golist stdout so seed can start while the authoritative `go list`
/// subprocess is in flight. Writes remain gated by [`golist_cache_enabled`].
fn try_peek_golist_cache(cfg: &Config, patterns: &[String], args: &[String]) -> Option<String> {
    for key in ["GUFF_CACHE", "GOLANGCI_LINT_CACHE"] {
        if let Ok(v) = std::env::var(key) {
            if v == "off" {
                return None;
            }
        }
    }
    // Avoid the X-1 `.go` filename walk when there is nothing to peek.
    let dir = guff_cache_dir()?;
    if !dir.join("golist").is_dir() {
        return None;
    }
    let key = golist_cache_key(cfg, patterns, args, true);
    let path = golist_cache_path(&key)?;
    std::fs::read_to_string(path).ok()
}

/// Read-only stdlib-export map from disk (no subprocess). `None` on miss.
fn peek_stdlib_export_cache(
    cfg: &Config,
    paths: &[String],
) -> Option<HashMap<String, String>> {
    for key in ["GUFF_CACHE", "GOLANGCI_LINT_CACHE"] {
        if let Ok(v) = std::env::var(key) {
            if v == "off" {
                return None;
            }
        }
    }
    let path = stdlib_export_cache_path(cfg, paths)?;
    load_stdlib_export_cache(&path)
}

/// Package graph recovered from a previous run's golist + stdlib-export caches.
pub struct PeekedGraph {
    pub roots: Vec<String>,
    pub packages: Vec<Arc<Package>>,
}

/// Best-effort read of the golist disk caches for C-7 speculation.
///
/// Ignores `disable_cache`. Returns `Ok(None)` when there is no usable cache
/// (or stdlib exports are missing — without them the seed fingerprint would
/// almost always miss against the authoritative fetch).
pub fn peek_cached_graph(
    cfg: &Config,
    patterns: &[String],
) -> Result<Option<PeekedGraph>, GoListError> {
    let args = golist_args(cfg, patterns, go_minor_version(cfg));
    let Some(stdout) = try_peek_golist_cache(cfg, patterns, &args) else {
        return Ok(None);
    };
    if !export_paths_exist(&stdout) {
        return Ok(None);
    }

    let mut roots = Vec::new();
    let mut packages = Vec::new();
    let mut seen: HashMap<String, JsonPackage> = HashMap::new();
    let stream = serde_json::Deserializer::from_str(&stdout).into_iter::<JsonPackage>();
    for item in stream {
        let p = item.map_err(|e| GoListError::Json(e.to_string()))?;
        if p.import_path.is_empty() {
            return Ok(None);
        }
        if seen.contains_key(&p.import_path) {
            continue;
        }
        seen.insert(p.import_path.clone(), p.clone());
        let pkg = json_package_to_package(&p, cfg)?;
        if !p.dep_only {
            roots.push(pkg.id.clone());
        }
        packages.push(Arc::new(pkg));
    }
    if packages.is_empty() {
        return Ok(None);
    }

    if cfg.dep_source {
        let stdlib: Vec<String> = seen
            .values()
            .filter(|p| p.standard && p.import_path != "unsafe" && p.error.is_none())
            .map(|p| p.import_path.clone())
            .collect();
        if !stdlib.is_empty() {
            let Some(exports) = peek_stdlib_export_cache(cfg, &stdlib) else {
                return Ok(None);
            };
            for pkg in packages.iter_mut() {
                if let Some(export) = exports.get(&pkg.id) {
                    if !export.is_empty() && Path::new(export).exists() {
                        Arc::make_mut(pkg).export_file = PathBuf::from(export);
                    }
                }
            }
        }

        // Speculation is only useful when it reproduces the authoritative
        // graph, so a cgo package whose `CompiledGoFiles` is not already
        // cached means the fingerprint would miss anyway — give up instead of
        // seeding from the `GoFiles` fallback.
        let want_compiled = if defers_compiled(cfg) {
            needs_compiled_query(&seen)
        } else {
            Vec::new()
        };
        if !want_compiled.is_empty() {
            let Some(compiled) = peek_compiled_files_cache(cfg, &want_compiled) else {
                return Ok(None);
            };
            attach_compiled_files(&mut packages, &compiled);
        }
        // Default dep-export reuse is Off; when CachedOnly/Fetch is on, attach
        // whatever the dep-export cache already holds (no subprocess).
        let third_party: Vec<String> = seen
            .values()
            .filter(|p| {
                !p.standard
                    && p.error.is_none()
                    && p.import_path != "unsafe"
                    && !p.import_path.contains(' ')
                    && !p.import_path.ends_with(".test")
                    && p.module.as_ref().is_some_and(|m| !m.main)
            })
            .map(|p| p.import_path.clone())
            .collect();
        if !third_party.is_empty() {
            let mode = export_reuse_mode();
            if mode != ExportReuseMode::Off {
                let exports = load_dep_export_cache(cfg, &third_party);
                for pkg in packages.iter_mut() {
                    if let Some(export) = exports.get(&pkg.id) {
                        if !export.is_empty() && Path::new(export).exists() {
                            Arc::make_mut(pkg).export_file = PathBuf::from(export);
                        }
                    }
                }
            }
        }
    }

    Ok(Some(PeekedGraph { roots, packages }))
}

fn store_golist_cache(cfg: &Config, patterns: &[String], args: &[String], stdout: &str) {
    if !golist_cache_enabled(cfg) {
        return;
    }
    let key = golist_cache_key(cfg, patterns, args, true);
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

    let wants_compiled = mode.contains(LoadMode::NEED_COMPILED_GO_FILES)
        || mode.contains(LoadMode::NEED_SYNTAX)
        || mode.contains(LoadMode::NEED_TYPES)
        || mode.contains(LoadMode::NEED_TYPES_INFO)
        || mode.contains(LoadMode::NEED_TYPES_SIZES);

    let mut args = vec![
        "list".to_string(),
        "-e".to_string(),
        json_flag(cfg, go_version),
        format!("-compiled={}", wants_compiled && !defers_compiled(cfg)),
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

/// Every JSON field [`JsonPackage`] decodes.
///
/// Kept in sync with that struct by `json_flag_requests_every_decoded_field`,
/// which fails the build's test run if a `#[serde(rename)]` is added there and
/// not here.
const DESERIALIZED_FIELDS: &[&str] = &[
    "ImportPath",
    "Dir",
    "Name",
    "Target",
    "Export",
    "GoFiles",
    "CompiledGoFiles",
    "IgnoredGoFiles",
    "IgnoredOtherFiles",
    "EmbedPatterns",
    "EmbedFiles",
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
    "Imports",
    "ImportMap",
    "Deps",
    "Module",
    "ForTest",
    "DepOnly",
    "Standard",
    "Error",
];

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

    // Everything `JsonPackage` decodes, unconditionally.
    //
    // The mode-driven additions below mirror golangci-lint and decide what
    // `go list` should *compute*. They are not a safe answer to what guff
    // *reads*: `json_package_to_package` fills `for_test`, `target`, the embed
    // lists and the `other_files` group regardless of `LoadMode`, and
    // `guff-analysis`'s `Pass` reads `other_files` / `ignored_files`. Under a
    // bare `-json` that mismatch is invisible because every field is present;
    // under `-json=<fields>` an omission turns into a silently empty vector and
    // a findings change nobody can trace back to here.
    add(DESERIALIZED_FIELDS);
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

/// Whether `CompiledGoFiles` is resolved by a second, cgo-restricted `go list`
/// instead of by `-compiled=true` on the main call (PERF_TASKS_V2 C-3a).
///
/// `-compiled=true` makes cmd/go build the action graph for **every** package
/// in the answer, which measured 0.39s of the main call's 0.90s on prometheus
/// `./...` — and it is not cgo work (`CGO_ENABLED=0` costs the same). What it
/// buys is almost nothing: `CompiledGoFiles == GoFiles` for 1527 of those 1530
/// packages. The three exceptions are `unsafe` (already special-cased in
/// [`json_package_to_package`]) and the two packages holding `CgoFiles`.
///
/// So run the main call with `-compiled=false`, let the existing
/// empty-`CompiledGoFiles` fallback fill in `GoFiles`, and ask a second
/// `go list -compiled=true` about the cgo/SWIG packages only — a set the
/// first call's own `CgoFiles`/`SwigFiles` output identifies.
///
/// Only for the hybrid source mode: the export path passes `-export=true`,
/// which builds the action graph regardless, so deferring would add a
/// subprocess and save nothing.
fn defers_compiled(cfg: &Config) -> bool {
    cfg.dep_source
}

/// Packages whose `CompiledGoFiles` cannot be derived from `GoFiles`.
///
/// Test-variant ids (`pkg [pkg.test]`) are excluded: they are not valid
/// `go list` arguments and would abort the whole batch.
fn needs_compiled_query(seen: &HashMap<String, JsonPackage>) -> Vec<String> {
    let mut out: Vec<String> = seen
        .values()
        .filter(|p| {
            (!p.cgo_files.is_empty()
                || !p.swig_files.is_empty()
                || !p.swig_cxx_files.is_empty())
                && p.error.is_none()
                && !p.import_path.contains(' ')
        })
        .map(|p| p.import_path.clone())
        .collect();
    out.sort();
    out
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
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

/// Minimal shape for the cgo/SWIG-restricted `go list -compiled=true` (C-3a).
#[derive(Debug, Clone, Deserialize)]
struct JsonCompiled {
    #[serde(rename = "ImportPath")]
    import_path: String,
    #[serde(default, rename = "Dir")]
    dir: String,
    #[serde(default, rename = "CompiledGoFiles")]
    compiled_go_files: Vec<String>,
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

    /// Extract the `#[serde(rename = "…")]` names of one struct in this file.
    fn serde_renames_of(struct_name: &str) -> Vec<String> {
        let src = include_str!("golist.rs");
        let start = src
            .find(&format!("struct {struct_name} {{"))
            .unwrap_or_else(|| panic!("{struct_name} not found"));
        let body = &src[start..];
        let end = body.find("\n}").expect("struct end");
        body[..end]
            .match_indices("rename = \"")
            .map(|(i, m)| {
                let rest = &body[i + m.len()..];
                rest[..rest.find('"').expect("closing quote")].to_string()
            })
            .collect()
    }

    /// `-json=<fields>` only asks for what we list, but `json_package_to_package`
    /// reads whatever `JsonPackage` can decode. A field added to the struct and
    /// not to `DESERIALIZED_FIELDS` would deserialize to its default forever,
    /// which no type error and no compiler warning would catch.
    #[test]
    fn json_flag_requests_every_decoded_field() {
        let decoded = serde_renames_of("JsonPackage");
        assert!(
            decoded.len() > 20,
            "extraction looks broken, got {decoded:?}"
        );
        let requested: std::collections::HashSet<&str> =
            DESERIALIZED_FIELDS.iter().copied().collect();
        let missing: Vec<&String> = decoded
            .iter()
            .filter(|f| !requested.contains(f.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "JsonPackage decodes {missing:?} but json_flag never asks go list \
             for them; add them to DESERIALIZED_FIELDS"
        );

        let decoded_set: std::collections::HashSet<&str> =
            decoded.iter().map(String::as_str).collect();
        let stale: Vec<&&str> = DESERIALIZED_FIELDS
            .iter()
            .filter(|f| !decoded_set.contains(**f))
            .collect();
        assert!(
            stale.is_empty(),
            "DESERIALIZED_FIELDS lists {stale:?}, which JsonPackage no longer decodes"
        );
    }

    #[test]
    fn json_flag_selects_fields_from_go_119() {
        let cfg = Config {
            mode: LoadMode::LOAD_ALL_SYNTAX,
            ..Config::default()
        };
        // Pre-1.19 toolchains have no field selection; asking for one would
        // make `go list` treat it as a pattern.
        assert_eq!(json_flag(&cfg, 18), "-json");
        let flag = json_flag(&cfg, 26);
        assert!(flag.starts_with("-json="));
        for f in DESERIALIZED_FIELDS {
            assert!(flag.contains(f), "{flag} is missing {f}");
        }
    }

    #[test]
    fn dep_source_defers_compiled_to_the_cgo_query() {
        let hybrid = Config {
            mode: LoadMode::NEED_COMPILED_GO_FILES,
            dep_source: true,
            ..Config::default()
        };
        assert!(golist_args(&hybrid, &[], 26).contains(&"-compiled=false".to_string()));

        // The export path builds the action graph anyway, so deferring there
        // would buy a subprocess and no time.
        let export = Config {
            dep_source: false,
            ..hybrid
        };
        assert!(golist_args(&export, &[], 26).contains(&"-compiled=true".to_string()));
    }

    #[test]
    fn needs_compiled_query_selects_only_cgo_and_swig() {
        let pkg = |path: &str, cgo: &[&str], swig: &[&str]| JsonPackage {
            import_path: path.to_string(),
            cgo_files: cgo.iter().map(|s| (*s).to_string()).collect(),
            swig_files: swig.iter().map(|s| (*s).to_string()).collect(),
            ..JsonPackage::default()
        };
        let mut seen = HashMap::new();
        for p in [
            pkg("plain", &[], &[]),
            pkg("withcgo", &["c.go"], &[]),
            pkg("withswig", &[], &["s.swig"]),
            // Test variants are not valid `go list` arguments.
            pkg("withcgo [withcgo.test]", &["c.go"], &[]),
        ] {
            seen.insert(p.import_path.clone(), p);
        }
        assert_eq!(
            needs_compiled_query(&seen),
            vec!["withcgo".to_string(), "withswig".to_string()]
        );
    }

    #[test]
    fn compiled_files_cache_roundtrips_and_rejects_unresolvable_paths() {
        let tmp = std::env::temp_dir().join(format!(
            "guff-compiled-files-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).expect("mkdir");
        let path = tmp.join("compiled.json");

        let src = tmp.join("a.go");
        let generated = tmp.join("_cgo_gotypes.go");
        std::fs::write(&src, b"package a").expect("a.go");
        std::fs::write(&generated, b"package a").expect("generated");
        let mut map = HashMap::new();
        map.insert(
            "example.com/a".to_string(),
            vec![
                src.display().to_string(),
                generated.display().to_string(),
            ],
        );

        store_compiled_files_cache(&path, &map);
        assert_eq!(load_compiled_files_cache(&path).as_ref(), Some(&map));

        // `go list -compiled` names a package's own sources relative to `Dir`.
        // Storing them that way makes the existence check depend on the
        // process's working directory, so the entry can never be validated —
        // which silently turns every warm run back into a subprocess.
        let mut relative = HashMap::new();
        relative.insert("example.com/a".to_string(), vec!["a.go".to_string()]);
        store_compiled_files_cache(&path, &relative);
        assert!(
            load_compiled_files_cache(&path).is_none(),
            "relative paths must not validate"
        );

        // Generated files live in GOCACHE, which is cleaned independently.
        store_compiled_files_cache(&path, &map);
        std::fs::remove_file(&generated).expect("rm generated");
        assert!(load_compiled_files_cache(&path).is_none());

        // Corrupt / empty content is a miss, never a panic.
        std::fs::write(&path, b"{\"a\": [\"/tmp/tr").expect("truncated");
        assert!(load_compiled_files_cache(&path).is_none());
        std::fs::write(&path, b"{}").expect("empty");
        assert!(load_compiled_files_cache(&path).is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn go_minor_version_parses_goroot_version_file() {
        let tmp = std::env::temp_dir().join(format!(
            "guff-goroot-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).expect("mkdir");
        std::fs::write(tmp.join("VERSION"), "go1.26.4\ntime 2026-05-29T15:26:39Z\n")
            .expect("VERSION");
        assert_eq!(parse_goroot_version(&tmp), Some(26));

        // Pre-release suffixes and a missing/garbled file must not panic.
        std::fs::write(tmp.join("VERSION"), "go1.21rc1\n").expect("VERSION");
        assert_eq!(parse_goroot_version(&tmp), Some(21));
        std::fs::write(tmp.join("VERSION"), "devel +abcdef\n").expect("VERSION");
        assert_eq!(parse_goroot_version(&tmp), None);
        std::fs::remove_file(tmp.join("VERSION")).expect("rm");
        assert_eq!(parse_goroot_version(&tmp), None);

        // An explicit GOROOT in the config wins over PATH discovery.
        std::fs::write(tmp.join("VERSION"), "go1.22.0\n").expect("VERSION");
        let cfg = Config {
            env: Some(vec![format!("GOROOT={}", tmp.display())]),
            ..Config::default()
        };
        assert_eq!(go_minor_version(&cfg), 22);

        let _ = std::fs::remove_dir_all(&tmp);
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
        let k1 = golist_cache_key(&cfg, &pats, &args, true);
        let k2 = golist_cache_key(&cfg, &pats, &args, true);
        assert_eq!(k1, k2);
        assert_eq!(k1.len(), 64);

        let cfg2 = Config {
            tests: true,
            ..cfg.clone()
        };
        let args2 = golist_args(&cfg2, &pats, 0);
        let k3 = golist_cache_key(&cfg2, &pats, &args2, true);
        assert_ne!(k1, k3);

        let cfg3 = Config {
            disable_cache: true,
            ..cfg
        };
        assert!(!golist_cache_enabled(&cfg3));
    }

    #[test]
    fn golist_cache_key_tracks_go_file_set_not_contents() {
        // X-1: package add/remove must change the key; editing an existing
        // file's contents must not (issue cache handles that).
        let tmp = std::env::temp_dir().join(format!(
            "guff-golist-key-x1-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("pkg")).expect("mkdir");
        std::fs::write(tmp.join("go.mod"), "module example.com/x1\n\ngo 1.22\n").expect("go.mod");
        std::fs::write(tmp.join("pkg/a.go"), "package pkg\n").expect("a.go");

        let cfg = Config {
            mode: LoadMode::LOAD_IMPORTS,
            dir: tmp.clone(),
            disable_cache: false,
            ..Config::default()
        };
        let pats = vec!["./...".to_string()];
        let args = golist_args(&cfg, &pats, 0);
        let before = golist_cache_key(&cfg, &pats, &args, true);

        // Content edit of an existing file → same key.
        std::fs::write(tmp.join("pkg/a.go"), "package pkg\n\nfunc A() {}\n").expect("edit");
        let after_edit = golist_cache_key(&cfg, &pats, &args, true);
        assert_eq!(before, after_edit, "content edit must not change golist key");

        // New package → different key.
        std::fs::create_dir_all(tmp.join("newpkg")).expect("mkdir new");
        std::fs::write(tmp.join("newpkg/x.go"), "package newpkg\n").expect("new");
        let after_add = golist_cache_key(&cfg, &pats, &args, true);
        assert_ne!(before, after_add, "new package must change golist key");

        // Delete the new package → back to the original key.
        std::fs::remove_dir_all(tmp.join("newpkg")).expect("rm new");
        let after_del = golist_cache_key(&cfg, &pats, &args, true);
        assert_eq!(before, after_del, "deleting the new package must restore key");

        // include_go_files=false must ignore the file set (stdlib/dep-export).
        std::fs::create_dir_all(tmp.join("other")).expect("mkdir other");
        std::fs::write(tmp.join("other/y.go"), "package other\n").expect("other");
        let without = golist_cache_key(&cfg, &pats, &args, false);
        let without2 = golist_cache_key(&cfg, &pats, &args, false);
        assert_eq!(without, without2);
        assert_ne!(
            golist_cache_key(&cfg, &pats, &args, true),
            without,
            "include_go_files must actually affect the key"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn collect_go_files_skips_vendor_testdata_and_dot_dirs() {
        let tmp = std::env::temp_dir().join(format!(
            "guff-golist-walk-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("keep")).expect("mkdir");
        std::fs::create_dir_all(tmp.join("vendor/x")).expect("vendor");
        std::fs::create_dir_all(tmp.join("testdata")).expect("testdata");
        std::fs::create_dir_all(tmp.join(".git")).expect(".git");
        std::fs::create_dir_all(tmp.join("_hidden")).expect("_hidden");
        std::fs::write(tmp.join("keep/a.go"), "package keep\n").unwrap();
        std::fs::write(tmp.join("vendor/x/v.go"), "package x\n").unwrap();
        std::fs::write(tmp.join("testdata/t.go"), "package testdata\n").unwrap();
        std::fs::write(tmp.join(".git/g.go"), "package g\n").unwrap();
        std::fs::write(tmp.join("_hidden/h.go"), "package h\n").unwrap();

        let mut files = Vec::new();
        collect_go_files_for_pattern(&tmp, None, "./...", None, &mut files);
        files.sort();
        assert_eq!(files, vec!["keep/a.go".to_string()]);

        let _ = std::fs::remove_dir_all(&tmp);
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
