//! Persistent per-package issues cache (golangci-lint `internal/cache` + issues store).
//!
//! Cache key = package source/deps hash + salt (guff version, analyzers, settings,
//! build tags, Go version). Entries are JSON under `$GUFF_CACHE` /
//! `$GOLANGCI_LINT_CACHE` / `{UserCacheDir}/guff`.
//!
//! DEFERRED: facts persistence (`runner_action_cache.go`); load/typecheck skip.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use guff::position::FileSet;
use guff_analysis::Diagnostic;
use guff_packages::Package;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Environment variables consulted by [`default_cache_dir`] (first wins).
pub const ENV_GUFF_CACHE: &str = "GUFF_CACHE";
pub const ENV_GOLANGCI_LINT_CACHE: &str = "GOLANGCI_LINT_CACHE";

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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CachedEntry {
    diagnostics: Vec<CachedDiagnostic>,
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
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn salt(&self) -> &str {
        &self.salt
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
        let mut files: Vec<PathBuf> = pkg.compiled_go_files.clone();
        files.extend(pkg.ignored_files.iter().cloned());
        files.sort();

        let mut h_self = Sha256::new();
        h_self.update(b"package hash\n");
        h_self.update(format!("pkgpath {}\n", pkg.pkg_path).as_bytes());
        for f in &files {
            let fh = self.file_hash(f)?;
            let display = f.to_string_lossy();
            h_self.update(format!("file {display} {}\n", hex_encode(&fh)).as_bytes());
        }
        let self_hash = hex_encode(&h_self.finalize());

        let mut imps: Vec<_> = pkg.imports.values().cloned().collect();
        imps.sort_by(|a, b| a.pkg_path.cmp(&b.pkg_path));

        let mut h_direct = Sha256::new();
        h_direct.update(format!("self {self_hash}\n").as_bytes());
        for dep in &imps {
            if dep.pkg_path == "unsafe" {
                continue;
            }
            let dep_hash = self.package_hash(dep, HashMode::NeedOnlySelf)?;
            h_direct.update(format!("import {} {}\n", dep.pkg_path, dep_hash).as_bytes());
        }
        let direct_hash = hex_encode(&h_direct.finalize());

        let mut h_all = Sha256::new();
        h_all.update(format!("self {self_hash}\n").as_bytes());
        for dep in &imps {
            if dep.pkg_path == "unsafe" {
                continue;
            }
            let dep_hash = self.package_hash(dep, HashMode::NeedAllDeps)?;
            h_all.update(format!("import {} {}\n", dep.pkg_path, dep_hash).as_bytes());
        }
        let all_hash = hex_encode(&h_all.finalize());

        let mut out = HashMap::new();
        out.insert(HashMode::NeedOnlySelf as u8, self_hash);
        out.insert(HashMode::NeedDirectDeps as u8, direct_hash);
        out.insert(HashMode::NeedAllDeps as u8, all_hash);
        Ok(out)
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
    bytes
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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
}
