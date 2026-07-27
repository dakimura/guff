//! Persistent per-package issues + facts cache (golangci-lint `internal/cache`
//! + issues store + `runner_action_cache.go`).
//!
//! Cache key = package source/deps hash + salt (guff version, analyzers, settings,
//! build tags, Go version). Entries are JSON under `$GUFF_CACHE` /
//! `$GOLANGCI_LINT_CACHE` / `{UserCacheDir}/guff`.
//!
//! GOCACHE (Go build cache) is resolved separately via [`default_go_cache_dir`]
//! and injected into `go list` subprocesses (PL07). Diagnostics whose paths
//! fall under GOCACHE (cgo preprocessed files) are filtered by the lint
//! pipeline.
//!
//! Facts are stored per analyzer under `facts/` (R24). Sub-package incremental
//! typecheck / export-data sharing / `go list` metadata cache remain DEFERRED
//! (R24 items 2–4).

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use guff::position::FileSet;
use guff_analysis::{Diagnostic, EncodedFact};
use guff_packages::Package;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Environment variables consulted by [`default_cache_dir`] (first wins).
pub const ENV_GUFF_CACHE: &str = "GUFF_CACHE";
pub const ENV_GOLANGCI_LINT_CACHE: &str = "GOLANGCI_LINT_CACHE";
/// Go build cache (`go env GOCACHE`); used by `go list -export` and cgo.
pub const ENV_GOCACHE: &str = "GOCACHE";

/// How deeply import hashes fold into a package action id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashMode {
    /// Only this package's files.
    NeedOnlySelf,
    /// Self + direct imports' self-hashes.
    NeedDirectDeps,
    /// Self + transitive dependency hashes (golangci default for issues).
    NeedAllDeps,
}

#[derive(Debug)]
pub enum CacheError {
    Disabled(String),
    Io(io::Error),
    Message(String),
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled(msg) => write!(f, "{msg}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Message(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for CacheError {}

impl From<io::Error> for CacheError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Resolve the on-disk cache directory.
///
/// Precedence: `GUFF_CACHE` → `GOLANGCI_LINT_CACHE` → `{UserCacheDir}/guff`.
/// The value `"off"` disables the cache.
pub fn default_cache_dir() -> Result<PathBuf, CacheError> {
    for key in [ENV_GUFF_CACHE, ENV_GOLANGCI_LINT_CACHE] {
        if let Ok(v) = env::var(key) {
            if v.is_empty() {
                continue;
            }
            if v == "off" {
                return Err(CacheError::Disabled(format!("{key}=off")));
            }
            let p = PathBuf::from(&v);
            if !p.is_absolute() {
                return Err(CacheError::Message(format!(
                    "{key} must be an absolute path, got {v:?}"
                )));
            }
            return Ok(p);
        }
    }
    let base = user_cache_dir().ok_or_else(|| {
        CacheError::Message(
            "could not locate a user cache directory; set GUFF_CACHE".into(),
        )
    })?;
    Ok(base.join("guff"))
}

fn user_cache_dir() -> Option<PathBuf> {
    if let Ok(xdg) = env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg));
        }
    }
    #[cfg(target_os = "macos")]
    {
        return env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Caches"));
    }
    #[cfg(target_os = "windows")]
    {
        return env::var_os("LOCALAPPDATA").map(PathBuf::from);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache"));
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

/// Resolve the Go build cache directory (`GOCACHE`).
///
/// Precedence: `GOCACHE` env → `go env GOCACHE` → `{UserCacheDir}/go-build`.
/// Returns an error when the value is `"off"` (Go disables the build cache).
pub fn default_go_cache_dir() -> Result<PathBuf, CacheError> {
    if let Ok(v) = env::var(ENV_GOCACHE) {
        if v == "off" {
            return Err(CacheError::Disabled("GOCACHE=off".into()));
        }
        if !v.is_empty() {
            let p = PathBuf::from(&v);
            if !p.is_absolute() {
                return Err(CacheError::Message(format!(
                    "GOCACHE must be an absolute path, got {v:?}"
                )));
            }
            return Ok(p);
        }
    }
    if let Some(from_go) = detect_go_cache_from_go_env() {
        return Ok(from_go);
    }
    let base = user_cache_dir().ok_or_else(|| {
        CacheError::Message("could not locate a user cache directory; set GOCACHE".into())
    })?;
    Ok(base.join("go-build"))
}

fn detect_go_cache_from_go_env() -> Option<PathBuf> {
    let output = std::process::Command::new("go")
        .args(["env", "GOCACHE"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() || s == "off" {
        return None;
    }
    let p = PathBuf::from(s);
    p.is_absolute().then_some(p)
}

/// Ensure `GOCACHE` is present in a `KEY=value` environment list for `go list`.
///
/// When missing, inserts the resolved [`default_go_cache_dir`] so export-data
/// and cgo artifacts land in a known location (PL07).
pub fn ensure_go_cache_env(env_vars: &mut Vec<String>) {
    let has = env_vars.iter().any(|e| e.starts_with("GOCACHE="));
    if has {
        return;
    }
    if let Ok(dir) = default_go_cache_dir() {
        env_vars.push(format!("GOCACHE={}", dir.display()));
    }
}

/// Reports whether `path` is under the Go build cache (cgo / preprocessed files).
///
/// Equivalent to golangci-lint's `Cgo` processor path check.
pub fn is_under_go_cache(path: &Path, go_cache: Option<&Path>) -> bool {
    let Some(cache) = go_cache else {
        return false;
    };
    if cache.as_os_str().is_empty() {
        return false;
    }
    let path_str = path.to_string_lossy();
    let cache_str = cache.to_string_lossy();
    if path_str.starts_with(cache_str.as_ref()) {
        return true;
    }
    // Also reject the well-known cgo types file by basename.
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "_cgo_gotypes.go")
}

/// Remove the entire cache directory (best-effort recreate of parents is caller's job).
pub fn clean_cache(dir: &Path) -> Result<(), CacheError> {
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// Total size in bytes of files under `dir` (0 if missing).
pub fn cache_dir_size(dir: &Path) -> u64 {
    if !dir.exists() {
        return 0;
    }
    let mut total = 0u64;
    let Ok(walker) = walkdir(dir) else {
        return 0;
    };
    for path in walker {
        if let Ok(meta) = fs::metadata(&path) {
            if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

fn walkdir(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        for entry in fs::read_dir(&cur)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    Ok(out)
}

/// Stable fingerprint for cache salt (analyzers + config + tool/Go versions).
pub fn build_salt(
    guff_version: &str,
    analyzer_names: &[&str],
    build_tags: &[String],
    settings_fingerprint: &str,
    go_version: &str,
) -> String {
    let mut names: Vec<&str> = analyzer_names.to_vec();
    names.sort_unstable();
    let mut tags = build_tags.to_vec();
    tags.sort();
    format!(
        "guff={guff_version}\ngo={go_version}\nanalyzers={}\ntags={}\nsettings={settings_fingerprint}\n",
        names.join(","),
        tags.join(",")
    )
}

/// Best-effort `go env GOVERSION` (empty when `go` is unavailable).
pub fn detect_go_version() -> String {
    let output = std::process::Command::new("go")
        .args(["env", "GOVERSION"])
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

/// One diagnostic stored relative to a filename (not FileSet-absolute Pos).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CachedDiagnostic {
    pub analyzer: String,
    pub filename: String,
    pub offset: i64,
    pub end_offset: i64,
    pub line: i64,
    pub column: i64,
    pub category: String,
    pub message: String,
    pub url: String,
    #[serde(default)]
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CachedEntry {
    diagnostics: Vec<CachedDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CachedFactsEntry {
    facts: Vec<EncodedFact>,
}

/// Hit/miss counters for one runner invocation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub hit_packages: Vec<String>,
    pub miss_packages: Vec<String>,
}

/// File-backed issues cache keyed by package content hash + salt.
pub struct IssueCache {
    dir: PathBuf,
    salt: String,
    file_hashes: Mutex<HashMap<PathBuf, [u8; 32]>>,
    pkg_hashes: Mutex<HashMap<String, HashMap<u8, String>>>,
    /// `import path`/`id` → self-hash for every loaded package (roots + deps).
    ///
    /// Populated once via [`Self::set_dep_hashes`] so transitive dependency
    /// hashing (`NeedAllDeps`) can resolve each entry in the flat, complete,
    /// deterministic `Package::deps` list — instead of walking the in-memory
    /// import graph, whose depth is nondeterministic (import stubs are cloned
    /// at inconsistent resolution depth during loading, which flipped every
    /// package between cache hit and miss run to run).
    dep_self_hashes: HashMap<String, String>,
}

impl IssueCache {
    pub fn open(dir: PathBuf, salt: impl Into<String>) -> Result<Self, CacheError> {
        fs::create_dir_all(&dir)?;
        let readme = dir.join("README");
        if !readme.exists() {
            let _ = fs::write(
                &readme,
                "This directory holds cached analysis results from guff.\n",
            );
        }
        Ok(Self {
            dir,
            salt: salt.into(),
            file_hashes: Mutex::new(HashMap::new()),
            pkg_hashes: Mutex::new(HashMap::new()),
            dep_self_hashes: HashMap::new(),
        })
    }

    /// Register the self-hash of every loaded package (roots and transitive
    /// dependencies) so `NeedAllDeps`/`NeedDirectDeps` hashing is deterministic
    /// and complete. Keyed by both `pkg_path` and `id` because `Package::deps`
    /// and `imports` reference packages by import path while `id` may carry a
    /// test suffix. Call once, before the cache is shared for reads/writes.
    ///
    /// When the dependency graph fingerprint (`graph_key`) matches a previous
    /// run, the registry is loaded from disk instead of re-hashing every
    /// package's sources (the dominant cost of warm `cache setup+partition`).
    pub fn set_dep_hashes(&mut self, packages: &[Arc<Package>]) -> Result<(), CacheError> {
        let t0 = std::time::Instant::now();
        let graph_key = graph_key_for_packages(packages)?;
        let path = self.dep_hash_registry_path(&graph_key);
        if let Some(map) = load_dep_hash_registry(&path) {
            self.dep_self_hashes = map;
            if std::env::var_os("GUFF_DEBUG_CACHE").is_some() {
                eprintln!(
                    "guff:   dep-hash registry hit ({:.2}s, {} entries, key={})",
                    t0.elapsed().as_secs_f64(),
                    self.dep_self_hashes.len(),
                    &graph_key[..graph_key.len().min(12)],
                );
            }
            return Ok(());
        }

        let t_hash = std::time::Instant::now();
        for pkg in packages {
            let h = self.self_hash(pkg)?;
            if !pkg.pkg_path.is_empty() {
                self.dep_self_hashes.insert(pkg.pkg_path.clone(), h.clone());
            }
            if !pkg.id.is_empty() {
                self.dep_self_hashes.insert(pkg.id.clone(), h);
            }
        }
        let hash_secs = t_hash.elapsed().as_secs_f64();
        // Best-effort persist; a failed write just means the next run recomputes.
        let _ = save_dep_hash_registry(&path, &self.dep_self_hashes);
        if std::env::var_os("GUFF_DEBUG_CACHE").is_some() {
            eprintln!(
                "guff:   dep-hash registry miss+store ({:.2}s hash, {:.2}s total, {} entries, key={})",
                hash_secs,
                t0.elapsed().as_secs_f64(),
                self.dep_self_hashes.len(),
                &graph_key[..graph_key.len().min(12)],
            );
        }
        Ok(())
    }

    fn dep_hash_registry_path(&self, graph_key: &str) -> PathBuf {
        self.dir.join(format!(
            "dep_hash_registry.v{DEP_HASH_REGISTRY_SCHEMA}.{graph_key}.bin"
        ))
    }

    /// Self-only content hash of a package (its own compiled files).
    ///
    /// Ignored/build-tag-excluded files are intentionally omitted (R24.2 I1):
    /// they do not affect typecheck of `compiled_go_files`, and build-tag
    /// changes already invalidate via the cache salt. Full per-file
    /// incremental typecheck remains DEFERRED (Checker is whole-package).
    pub fn self_hash(&self, pkg: &Package) -> Result<String, CacheError> {
        let mut files: Vec<PathBuf> = pkg.compiled_go_files.clone();
        files.sort();

        let mut h = Sha256::new();
        h.update(b"package hash\n");
        h.update(format!("pkgpath {}\n", pkg.pkg_path).as_bytes());
        for f in &files {
            let fh = self.file_hash(f)?;
            let display = f.to_string_lossy();
            h.update(format!("file {display} {}\n", hex_encode(&fh)).as_bytes());
        }
        Ok(hex_encode(&h.finalize()))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn salt(&self) -> &str {
        &self.salt
    }

    /// Load raw cached diagnostics for `pkg` (filename/line/column preserved as
    /// stored), without needing a `FileSet`. Used by the lazy path to skip
    /// parsing/type-checking entirely for packages that hit the cache — the
    /// stored positions are already resolved, so no `FileSet` remap is needed.
    pub fn get_cached(
        &self,
        pkg: &Package,
        mode: HashMode,
    ) -> Result<Vec<CachedDiagnostic>, CacheError> {
        let key = self.action_id(pkg, mode)?;
        let path = self.entry_path(&key);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(CacheError::Message("missing".into()));
            }
            Err(e) => return Err(e.into()),
        };
        let entry: CachedEntry =
            serde_json::from_slice(&bytes).map_err(|e| CacheError::Message(e.to_string()))?;
        Ok(entry.diagnostics)
    }

    /// Load cached diagnostics for `pkg`, rematerializing positions into `fset`.
    pub fn get(
        &self,
        pkg: &Package,
        fset: &FileSet,
        mode: HashMode,
    ) -> Result<Vec<(String, Diagnostic)>, CacheError> {
        let key = self.action_id(pkg, mode)?;
        let path = self.entry_path(&key);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(CacheError::Message("missing".into()));
            }
            Err(e) => return Err(e.into()),
        };
        let entry: CachedEntry =
            serde_json::from_slice(&bytes).map_err(|e| CacheError::Message(e.to_string()))?;
        let mut out = Vec::with_capacity(entry.diagnostics.len());
        for cd in entry.diagnostics {
            let pos = remap_pos(fset, &cd.filename, cd.offset).unwrap_or(0);
            let end = if cd.end_offset > 0 {
                remap_pos(fset, &cd.filename, cd.end_offset).unwrap_or(0)
            } else {
                0
            };
            let action_id = format!("{}@{}", cd.analyzer, pkg.pkg_path);
            out.push((
                action_id,
                Diagnostic {
                    pos,
                    end,
                    category: cd.category,
                    message: cd.message,
                    severity: cd.severity,
                    url: cd.url,
                    suggested_fixes: Vec::new(),
                    related: Vec::new(),
                },
            ));
        }
        Ok(out)
    }

    /// Store diagnostics for `pkg` using positions from `fset`.
    pub fn put(
        &self,
        pkg: &Package,
        fset: &FileSet,
        mode: HashMode,
        diagnostics: &[(String, Diagnostic)],
    ) -> Result<(), CacheError> {
        let key = self.action_id(pkg, mode)?;
        let mut cached = Vec::with_capacity(diagnostics.len());
        for (action_id, diag) in diagnostics {
            let analyzer = action_id
                .split('@')
                .next()
                .unwrap_or(action_id)
                .to_string();
            let (filename, offset, line, column) = if diag.pos != 0 {
                let pos = fset.position(guff::Pos(diag.pos as i64));
                (pos.filename, pos.offset, pos.line, pos.column)
            } else {
                (String::new(), 0, 0, 0)
            };
            let end_offset = if diag.end != 0 {
                fset.position(guff::Pos(diag.end as i64)).offset
            } else {
                0
            };
            cached.push(CachedDiagnostic {
                analyzer,
                filename,
                offset,
                end_offset,
                line,
                column,
                category: diag.category.clone(),
                message: diag.message.clone(),
                url: diag.url.clone(),
                severity: diag.severity.clone(),
            });
        }
        let entry = CachedEntry {
            diagnostics: cached,
        };
        let bytes =
            serde_json::to_vec(&entry).map_err(|e| CacheError::Message(e.to_string()))?;
        let path = self.entry_path(&key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Load persisted analyzer facts for `pkg` (golangci `loadPersistedFacts`).
    pub fn get_facts(
        &self,
        pkg: &Package,
        mode: HashMode,
        analyzer: &str,
    ) -> Result<Vec<EncodedFact>, CacheError> {
        let key = self.action_id(pkg, mode)?;
        let path = self.facts_entry_path(&key, analyzer);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(CacheError::Message("missing".into()));
            }
            Err(e) => return Err(e.into()),
        };
        let entry: CachedFactsEntry =
            serde_json::from_slice(&bytes).map_err(|e| CacheError::Message(e.to_string()))?;
        Ok(entry.facts)
    }

    /// Persist analyzer facts for `pkg` (golangci `persistFactsToCache`).
    pub fn put_facts(
        &self,
        pkg: &Package,
        mode: HashMode,
        analyzer: &str,
        facts: &[EncodedFact],
    ) -> Result<(), CacheError> {
        let key = self.action_id(pkg, mode)?;
        let entry = CachedFactsEntry {
            facts: facts.to_vec(),
        };
        let bytes =
            serde_json::to_vec(&entry).map_err(|e| CacheError::Message(e.to_string()))?;
        let path = self.facts_entry_path(&key, analyzer);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn facts_entry_path(&self, action_id_hex: &str, analyzer: &str) -> PathBuf {
        let prefix = action_id_hex.get(..2).unwrap_or("00");
        // Sanitize analyzer name for the filesystem (should already be a Go ident).
        let safe: String = analyzer
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
            .collect();
        self.dir
            .join("facts")
            .join(prefix)
            .join(format!("{action_id_hex}-{safe}.json"))
    }

    fn entry_path(&self, action_id_hex: &str) -> PathBuf {
        let prefix = action_id_hex.get(..2).unwrap_or("00");
        self.dir
            .join("issues")
            .join(prefix)
            .join(format!("{action_id_hex}.json"))
    }

    fn action_id(&self, pkg: &Package, mode: HashMode) -> Result<String, CacheError> {
        let pkg_hash = self.package_hash(pkg, mode)?;
        let mut h = Sha256::new();
        h.update(b"action ID\n");
        h.update(format!("pkgpath {}\n", pkg.pkg_path).as_bytes());
        h.update(format!("pkghash {pkg_hash}\n").as_bytes());
        h.update(format!("salt {}\n", self.salt).as_bytes());
        Ok(hex_encode(&h.finalize()))
    }

    fn package_hash(&self, pkg: &Package, mode: HashMode) -> Result<String, CacheError> {
        {
            let guard = self.pkg_hashes.lock().unwrap();
            if let Some(modes) = guard.get(&pkg.id) {
                if let Some(h) = modes.get(&(mode as u8)) {
                    return Ok(h.clone());
                }
            }
        }
        let computed = self.compute_pkg_hashes(pkg)?;
        let result = computed
            .get(&(mode as u8))
            .cloned()
            .ok_or_else(|| CacheError::Message("hash mode missing".into()))?;
        let mut guard = self.pkg_hashes.lock().unwrap();
        guard.insert(pkg.id.clone(), computed);
        Ok(result)
    }

    fn compute_pkg_hashes(&self, pkg: &Package) -> Result<HashMap<u8, String>, CacheError> {
        let self_hash = self.self_hash(pkg)?;

        // Direct imports: hash each direct dependency's self-hash. Resolve
        // through the registry by import path (stable and complete); fall back
        // to hashing the in-memory import stub only when the dependency was not
        // registered (e.g. `unsafe`, or a synthetic package).
        let mut direct: Vec<(String, String)> = Vec::new();
        for (path, dep) in &pkg.imports {
            if path == "unsafe" || dep.pkg_path == "unsafe" {
                continue;
            }
            let dep_hash = match self.lookup_dep_hash(path, dep) {
                Some(h) => h,
                None => self.self_hash(dep)?,
            };
            direct.push((path.clone(), dep_hash));
        }
        direct.sort();
        let mut h_direct = Sha256::new();
        h_direct.update(format!("self {self_hash}\n").as_bytes());
        for (path, dep_hash) in &direct {
            h_direct.update(format!("import {path} {dep_hash}\n").as_bytes());
        }
        let direct_hash = hex_encode(&h_direct.finalize());

        // All deps: fold in every transitive dependency's self-hash using the
        // flat, sorted, complete `deps` list from `go list`. This is fully
        // deterministic — unlike walking `imports`, whose graph depth varies
        // run to run. Dependencies missing from the registry contribute their
        // path only (still deterministic).
        let mut deps = pkg.deps.clone();
        deps.sort();
        deps.dedup();
        let mut h_all = Sha256::new();
        h_all.update(format!("self {self_hash}\n").as_bytes());
        for dep_path in &deps {
            if dep_path == "unsafe" {
                continue;
            }
            let dep_hash = self
                .dep_self_hashes
                .get(dep_path)
                .cloned()
                .unwrap_or_default();
            h_all.update(format!("dep {dep_path} {dep_hash}\n").as_bytes());
        }
        let all_hash = hex_encode(&h_all.finalize());

        let mut out = HashMap::new();
        out.insert(HashMode::NeedOnlySelf as u8, self_hash);
        out.insert(HashMode::NeedDirectDeps as u8, direct_hash);
        out.insert(HashMode::NeedAllDeps as u8, all_hash);
        Ok(out)
    }

    /// Resolve a direct dependency's self-hash from the registry (by import
    /// path, then by id), falling back to `None` when unregistered.
    fn lookup_dep_hash(&self, path: &str, dep: &Package) -> Option<String> {
        self.dep_self_hashes
            .get(path)
            .or_else(|| self.dep_self_hashes.get(&dep.pkg_path))
            .or_else(|| self.dep_self_hashes.get(&dep.id))
            .cloned()
    }

    fn file_hash(&self, path: &Path) -> Result<[u8; 32], CacheError> {
        {
            let guard = self.file_hashes.lock().unwrap();
            if let Some(h) = guard.get(path) {
                return Ok(*h);
            }
        }
        let bytes = fs::read(path)?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let dig: [u8; 32] = hasher.finalize().into();
        self.file_hashes
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), dig);
        Ok(dig)
    }
}

/// Schema version for the on-disk dep-hash registry. Bump when the fingerprint
/// inputs or the serialized map format change.
const DEP_HASH_REGISTRY_SCHEMA: u32 = 1;

/// Deterministic fingerprint of the loaded package graph + source file identity.
///
/// Includes package id/path/deps (from `go list`) and each compiled file's
/// path/len/mtime so a content edit invalidates the cached registry even when
/// the import graph is unchanged. Uses metadata rather than content hashes so
/// building the key stays cheap relative to `self_hash`.
fn graph_key_for_packages(packages: &[Arc<Package>]) -> Result<String, CacheError> {
    let mut pkgs: Vec<&Package> = packages.iter().map(|p| p.as_ref()).collect();
    pkgs.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.pkg_path.cmp(&b.pkg_path)));

    let mut h = Sha256::new();
    h.update(format!("dep-hash-registry v{DEP_HASH_REGISTRY_SCHEMA}\n").as_bytes());
    for pkg in pkgs {
        h.update(format!("pkg {} {}\n", pkg.id, pkg.pkg_path).as_bytes());
        let mut deps = pkg.deps.clone();
        deps.sort();
        deps.dedup();
        for dep in &deps {
            h.update(format!("dep {dep}\n").as_bytes());
        }
        let mut files = pkg.compiled_go_files.clone();
        files.sort();
        for f in &files {
            let meta = fs::metadata(f)?;
            let mtime_nanos = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            h.update(
                format!(
                    "file {} {} {}\n",
                    f.to_string_lossy(),
                    meta.len(),
                    mtime_nanos
                )
                .as_bytes(),
            );
        }
    }
    Ok(hex_encode(&h.finalize()))
}

#[derive(Debug, Serialize, Deserialize)]
struct DepHashRegistryFile {
    schema: u32,
    /// Sorted for stable on-disk representation (load order does not matter).
    entries: Vec<(String, String)>,
}

fn load_dep_hash_registry(path: &Path) -> Option<HashMap<String, String>> {
    let bytes = fs::read(path).ok()?;
    let parsed: DepHashRegistryFile = serde_json::from_slice(&bytes).ok()?;
    if parsed.schema != DEP_HASH_REGISTRY_SCHEMA {
        return None;
    }
    Some(parsed.entries.into_iter().collect())
}

fn save_dep_hash_registry(
    path: &Path,
    map: &HashMap<String, String>,
) -> Result<(), CacheError> {
    let mut entries: Vec<(String, String)> = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let file = DepHashRegistryFile {
        schema: DEP_HASH_REGISTRY_SCHEMA,
        entries,
    };
    let bytes =
        serde_json::to_vec(&file).map_err(|e| CacheError::Message(e.to_string()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Atomic-ish: write temp then rename so a crash mid-write cannot leave a
    // half-parsed registry that we'd treat as authoritative.
    let tmp = path.with_extension("bin.tmp");
    fs::write(&tmp, &bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn remap_pos(fset: &FileSet, filename: &str, offset: i64) -> Option<u32> {
    if filename.is_empty() {
        return None;
    }
    for file in fset.files() {
        if file.name() == filename {
            return Some(file.pos(offset).0 as u32);
        }
    }
    // Basename fallback when absolute paths differ across machines.
    let base = Path::new(filename)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    for file in fset.files() {
        let name = file.name();
        if Path::new(name).file_name().and_then(|s| s.to_str()) == Some(base) {
            return Some(file.pos(offset).0 as u32);
        }
    }
    None
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    // Lower-case, zero-padded hex via a lookup table — same output as
    // `format!("{b:02x}")` but without a per-byte String alloc + format machinery.
    // This is on the cache-key path (once per file/package hash), so it runs
    // thousands of times per invocation. Mirrors guff-fmt's fmt_cache::hex_encode.
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

/// Partition packages into cache hits and misses; load hit diagnostics.
pub fn load_from_cache(
    cache: &IssueCache,
    packages: &[Arc<Package>],
) -> (Vec<(String, Diagnostic)>, Vec<Arc<Package>>, CacheStats) {
    let mut cached = Vec::new();
    let mut to_analyze = Vec::new();
    let mut stats = CacheStats::default();
    for pkg in packages {
        let fset = match pkg.fset.as_ref() {
            Some(f) => f,
            None => {
                stats.misses += 1;
                stats.miss_packages.push(pkg.pkg_path.clone());
                to_analyze.push(Arc::clone(pkg));
                continue;
            }
        };
        match cache.get(pkg, fset, HashMode::NeedAllDeps) {
            Ok(diags) => {
                stats.hits += 1;
                stats.hit_packages.push(pkg.pkg_path.clone());
                cached.extend(diags);
            }
            Err(_) => {
                stats.misses += 1;
                stats.miss_packages.push(pkg.pkg_path.clone());
                to_analyze.push(Arc::clone(pkg));
            }
        }
    }
    (cached, to_analyze, stats)
}

/// Persist diagnostics for packages that were freshly analyzed.
pub fn save_to_cache(
    cache: &IssueCache,
    packages: &[Arc<Package>],
    diagnostics: &[(String, Diagnostic)],
) {
    let mut per_pkg: HashMap<String, Vec<(String, Diagnostic)>> = HashMap::new();
    for (action_id, diag) in diagnostics {
        let pkg_path = action_id.split('@').nth(1).unwrap_or("");
        per_pkg
            .entry(pkg_path.to_string())
            .or_default()
            .push((action_id.clone(), diag.clone()));
    }
    for pkg in packages {
        let Some(fset) = pkg.fset.as_ref() else {
            continue;
        };
        let diags = per_pkg.remove(&pkg.pkg_path).unwrap_or_default();
        let _ = cache.put(pkg, fset, HashMode::NeedAllDeps, &diags);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff::position::FileSet;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn hex_encode_matches_format() {
        // The lookup-table encoder must be byte-for-byte identical to the old
        // `format!("{b:02x}")`: cache keys are built from its output, so any
        // divergence would silently invalidate every cached package.
        for bytes in [
            vec![],
            vec![0x00],
            vec![0x0f],
            vec![0xff],
            vec![0x00, 0x0f, 0xff, 0xa5, 0x5a],
            (0u8..=255).collect::<Vec<_>>(),
        ] {
            let want: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            assert_eq!(hex_encode(&bytes), want, "mismatch for {bytes:?}");
        }
    }

    fn pkg_with_file(dir: &Path, name: &str, body: &str) -> Arc<Package> {
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        let fset = FileSet::new();
        let _file = fset.add_file(path.to_str().unwrap(), -1, body.len() as i64);
        Arc::new(Package {
            id: "example.com/p".into(),
            pkg_path: "example.com/p".into(),
            dir: dir.to_path_buf(),
            compiled_go_files: vec![path],
            fset: Some(fset),
            ..Package::default()
        })
    }

    #[test]
    fn facts_put_get_roundtrip() {
        use guff_analysis::{ensure_builtin_fact_decoders, EncodedFact};
        ensure_builtin_fact_decoders();
        let tmp = TempDir::new().unwrap();
        let cache = IssueCache::open(tmp.path().to_path_buf(), "salt-v1").unwrap();
        let pkg_dir = tmp.path().join("src");
        fs::create_dir_all(&pkg_dir).unwrap();
        let pkg = pkg_with_file(&pkg_dir, "a.go", "package p\n");
        let facts = vec![EncodedFact {
            pkg_path: String::new(),
            object_path: "V".into(),
            fact_type: "StringFact".into(),
            payload: serde_json::json!({ "s": "hi" }),
        }];
        cache
            .put_facts(&pkg, HashMode::NeedAllDeps, "deprecated", &facts)
            .unwrap();
        let loaded = cache
            .get_facts(&pkg, HashMode::NeedAllDeps, "deprecated")
            .unwrap();
        assert_eq!(loaded, facts);
        assert!(cache
            .get_facts(&pkg, HashMode::NeedAllDeps, "other")
            .is_err());
    }

    #[test]
    fn put_get_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let cache = IssueCache::open(tmp.path().to_path_buf(), "salt-v1").unwrap();
        let pkg_dir = tmp.path().join("src");
        fs::create_dir_all(&pkg_dir).unwrap();
        let pkg = pkg_with_file(&pkg_dir, "a.go", "package p\n");
        let fset = pkg.fset.as_ref().unwrap();
        let file = &fset.files()[0];
        let pos = file.pos(0).0 as u32;
        let diags = vec![(
            "errcheck@example.com/p".into(),
            Diagnostic {
                pos,
                message: "unchecked error".into(),
                ..Diagnostic::default()
            },
        )];
        cache
            .put(&pkg, fset, HashMode::NeedAllDeps, &diags)
            .unwrap();
        let loaded = cache.get(&pkg, fset, HashMode::NeedAllDeps).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].1.message, "unchecked error");
        assert_eq!(loaded[0].0, "errcheck@example.com/p");
    }

    #[test]
    fn content_change_misses() {
        let tmp = TempDir::new().unwrap();
        let cache = IssueCache::open(tmp.path().to_path_buf(), "salt-v1").unwrap();
        let pkg_dir = tmp.path().join("src");
        fs::create_dir_all(&pkg_dir).unwrap();
        let pkg = pkg_with_file(&pkg_dir, "a.go", "package p\n");
        let fset = pkg.fset.as_ref().unwrap();
        cache
            .put(&pkg, fset, HashMode::NeedAllDeps, &[])
            .unwrap();
        assert!(cache.get(&pkg, fset, HashMode::NeedAllDeps).is_ok());

        // New IssueCache instance (clears in-memory file hash) + mutated file.
        let cache2 = IssueCache::open(tmp.path().to_path_buf(), "salt-v1").unwrap();
        fs::write(pkg.compiled_go_files[0].as_path(), "package p\n// changed\n").unwrap();
        assert!(cache2.get(&pkg, fset, HashMode::NeedAllDeps).is_err());
    }

    #[test]
    fn salt_change_misses() {
        let tmp = TempDir::new().unwrap();
        let pkg_dir = tmp.path().join("src");
        fs::create_dir_all(&pkg_dir).unwrap();
        let pkg = pkg_with_file(&pkg_dir, "a.go", "package p\n");
        let fset = pkg.fset.as_ref().unwrap();
        let c1 = IssueCache::open(tmp.path().to_path_buf(), "salt-a").unwrap();
        c1.put(&pkg, fset, HashMode::NeedAllDeps, &[]).unwrap();
        let c2 = IssueCache::open(tmp.path().to_path_buf(), "salt-b").unwrap();
        assert!(c2.get(&pkg, fset, HashMode::NeedAllDeps).is_err());
    }

    #[test]
    fn clean_removes_dir() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("guff-cache");
        fs::create_dir_all(dir.join("issues")).unwrap();
        fs::write(dir.join("issues/x"), b"1").unwrap();
        clean_cache(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn is_under_go_cache_prefix_and_cgo_basename() {
        let cache = PathBuf::from("/tmp/gocache");
        assert!(is_under_go_cache(
            Path::new("/tmp/gocache/abc/_cgo_gotypes.go"),
            Some(&cache)
        ));
        assert!(is_under_go_cache(
            Path::new("/elsewhere/_cgo_gotypes.go"),
            Some(&cache)
        ));
        assert!(!is_under_go_cache(
            Path::new("/tmp/src/main.go"),
            Some(&cache)
        ));
        assert!(!is_under_go_cache(Path::new("/tmp/gocache/x.go"), None));
    }

    #[test]
    fn ensure_go_cache_env_injects_when_missing() {
        let mut env = vec!["PATH=/bin".into()];
        ensure_go_cache_env(&mut env);
        assert!(
            env.iter().any(|e| e.starts_with("GOCACHE=")),
            "expected GOCACHE injection, got {env:?}"
        );
        let before = env.clone();
        ensure_go_cache_env(&mut env);
        assert_eq!(env, before, "second call must be idempotent");
    }
}
