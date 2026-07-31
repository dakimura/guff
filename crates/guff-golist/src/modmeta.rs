//! Permanent per-module metadata cache for immutable modules (C-3c §④).
//!
//! GOMODCACHE packages are content-addressed (`module@version`) and GOROOT
//! packages are fixed per toolchain — once scanned for a given GOOS/GOARCH /
//! build-tags fingerprint they never need to be re-read. The main module is
//! never cached here (it changes).
//!
//! Phase 3 stores **one blob per `module@version`** (or stdlib VERSION), not
//! one JSON per package. Warm hits then pay ~N modules of syscalls instead of
//! ~N packages (~1489 → ~250 on prometheus).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use guff_build::{Context, Package as BuildPackage};

const MODMETA_VERSION: &str = "modmeta-v2";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPkgMeta {
    pub name: String,
    pub go_files: Vec<String>,
    pub cgo_files: Vec<String>,
    pub ignored_go_files: Vec<String>,
    pub test_go_files: Vec<String>,
    pub xtest_go_files: Vec<String>,
    pub imports: Vec<String>,
    pub test_imports: Vec<String>,
    pub xtest_imports: Vec<String>,
}

impl CachedPkgMeta {
    pub fn from_build(pkg: &BuildPackage) -> Self {
        Self {
            name: pkg.name.clone(),
            go_files: pkg.go_files.clone(),
            cgo_files: pkg.cgo_files.clone(),
            ignored_go_files: pkg.ignored_go_files.clone(),
            test_go_files: pkg.test_go_files.clone(),
            xtest_go_files: pkg.xtest_go_files.clone(),
            imports: pkg.imports.clone(),
            test_imports: pkg.test_imports.clone(),
            xtest_imports: pkg.xtest_imports.clone(),
        }
    }

    pub fn apply_to(&self, pkg: &mut BuildPackage) {
        pkg.name = self.name.clone();
        pkg.go_files = self.go_files.clone();
        pkg.cgo_files = self.cgo_files.clone();
        pkg.ignored_go_files = self.ignored_go_files.clone();
        pkg.test_go_files = self.test_go_files.clone();
        pkg.xtest_go_files = self.xtest_go_files.clone();
        pkg.imports = self.imports.clone();
        pkg.test_imports = self.test_imports.clone();
        pkg.xtest_imports = self.xtest_imports.clone();
    }
}

/// Cache key material for a **module** blob (all packages share one file).
pub struct ModMetaKey<'a> {
    pub module_path: &'a str,
    pub module_version: &'a str,
    pub pkg_path: &'a str,
    pub goos: &'a str,
    pub goarch: &'a str,
    pub build_tags: &'a [String],
    /// `true` for GOROOT packages (version key = toolchain identity).
    pub standard: bool,
    pub goroot_version: &'a str,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModuleBlob {
    version: String,
    /// import path → package metadata
    packages: HashMap<String, CachedPkgMeta>,
}

struct BlobState {
    packages: HashMap<String, CachedPkgMeta>,
    dirty: bool,
    path: PathBuf,
}

/// Session-scoped modmeta cache: load each module blob once, flush dirty blobs
/// at the end of a list walk.
pub struct ModMetaSession {
    blobs: Mutex<HashMap<String, BlobState>>,
}

impl ModMetaSession {
    pub fn new() -> Self {
        Self {
            blobs: Mutex::new(HashMap::new()),
        }
    }

    /// Import a package directory, using the module-level modmeta cache for
    /// immutable modules.
    ///
    /// `include_tests` controls whether `*_test.go` headers are opened. Only
    /// pattern roots need them (`list` passes `cfg.tests && is_root`); deps and
    /// cached GOMODCACHE/GOROOT packages never do.
    pub fn import_dir(
        &self,
        ctxt: &Context,
        dir: &Path,
        key: &ModMetaKey<'_>,
        include_tests: bool,
    ) -> Result<BuildPackage, guff_build::BuildError> {
        if !cache_enabled() || (!key.standard && key.module_version.is_empty()) {
            return ctxt.import_dir_with(dir, include_tests);
        }

        if let Some(meta) = self.lookup(key) {
            let mut pkg = BuildPackage {
                dir: dir.to_path_buf(),
                import_path: key.pkg_path.to_string(),
                goroot: key.standard,
                ..BuildPackage::default()
            };
            meta.apply_to(&mut pkg);
            return Ok(pkg);
        }

        // Immutable modules are never list roots, so tests are never needed.
        // Always scan without tests so the cached blob stays lean and matches
        // what dep walks ask for.
        let pkg = ctxt.import_dir_with(dir, false)?;
        self.insert(key, CachedPkgMeta::from_build(&pkg));
        Ok(pkg)
    }

    fn lookup(&self, key: &ModMetaKey<'_>) -> Option<CachedPkgMeta> {
        let hash = module_key_hash(key);
        let guard = self.blobs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = guard.get(&hash) {
            return state.packages.get(key.pkg_path).cloned();
        }
        drop(guard);

        let path = cache_path_for_hash(&hash)?;
        let loaded = load_blob(&path);
        let mut guard = self.blobs.lock().unwrap_or_else(|e| e.into_inner());
        // Another thread may have loaded while we read disk.
        if let Some(state) = guard.get(&hash) {
            return state.packages.get(key.pkg_path).cloned();
        }
        let packages = loaded.unwrap_or_default();
        let hit = packages.get(key.pkg_path).cloned();
        guard.insert(
            hash,
            BlobState {
                packages,
                dirty: false,
                path,
            },
        );
        hit
    }

    fn insert(&self, key: &ModMetaKey<'_>, meta: CachedPkgMeta) {
        let Some(path) = cache_path(key) else {
            return;
        };
        let hash = module_key_hash(key);
        let mut guard = self.blobs.lock().unwrap_or_else(|e| e.into_inner());
        let state = guard.entry(hash).or_insert_with(|| BlobState {
            packages: HashMap::new(),
            dirty: false,
            path,
        });
        state.packages.insert(key.pkg_path.to_string(), meta);
        state.dirty = true;
    }

    /// Persist any module blobs that gained new packages during this session.
    pub fn flush(&self) {
        if !cache_enabled() {
            return;
        }
        let mut guard = self.blobs.lock().unwrap_or_else(|e| e.into_inner());
        for state in guard.values_mut() {
            if !state.dirty {
                continue;
            }
            if let Some(parent) = state.path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let blob = ModuleBlob {
                version: MODMETA_VERSION.to_string(),
                packages: state.packages.clone(),
            };
            if let Ok(bytes) = serde_json::to_vec(&blob) {
                let _ = std::fs::write(&state.path, bytes);
                state.dirty = false;
            }
        }
    }
}

impl Default for ModMetaSession {
    fn default() -> Self {
        Self::new()
    }
}

fn cache_enabled() -> bool {
    for key in ["GUFF_CACHE", "GOLANGCI_LINT_CACHE"] {
        if let Ok(v) = std::env::var(key) {
            if v == "off" {
                return false;
            }
        }
    }
    true
}

fn cache_root() -> Option<PathBuf> {
    for key in ["GUFF_CACHE", "GOLANGCI_LINT_CACHE"] {
        if let Ok(v) = std::env::var(key) {
            if v.is_empty() || v == "off" {
                continue;
            }
            let p = PathBuf::from(&v);
            if p.is_absolute() {
                return Some(p.join("modmeta"));
            }
        }
    }
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("guff").join("modmeta"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        return std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join("Library/Caches/guff/modmeta")
        });
    }
    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("LOCALAPPDATA")
            .map(|h| PathBuf::from(h).join("guff").join("modmeta"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".cache/guff/modmeta"));
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

fn module_key_hash(key: &ModMetaKey<'_>) -> String {
    let mut h = Sha256::new();
    h.update(MODMETA_VERSION.as_bytes());
    h.update(b"\n");
    if key.standard {
        h.update(b"stdlib\n");
        h.update(key.goroot_version.as_bytes());
    } else {
        h.update(key.module_path.as_bytes());
        h.update(b"@");
        h.update(key.module_version.as_bytes());
    }
    h.update(b"\n");
    h.update(key.goos.as_bytes());
    h.update(b"/");
    h.update(key.goarch.as_bytes());
    h.update(b"\n");
    let mut tags = key.build_tags.to_vec();
    tags.sort();
    for t in tags {
        h.update(t.as_bytes());
        h.update(b",");
    }
    hex_encode(&h.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn cache_path(key: &ModMetaKey<'_>) -> Option<PathBuf> {
    cache_path_for_hash(&module_key_hash(key))
}

fn cache_path_for_hash(hash: &str) -> Option<PathBuf> {
    let root = cache_root()?;
    let prefix = hash.get(..2).unwrap_or("00");
    Some(root.join(prefix).join(format!("{hash}.json")))
}

fn load_blob(path: &Path) -> Option<HashMap<String, CachedPkgMeta>> {
    let bytes = std::fs::read(path).ok()?;
    let blob: ModuleBlob = serde_json::from_slice(&bytes).ok()?;
    if blob.version != MODMETA_VERSION {
        return None;
    }
    Some(blob.packages)
}

/// Read GOROOT/VERSION (or fallback string) for stdlib cache keys.
pub fn goroot_version(goroot: &Path) -> String {
    let path = goroot.join("VERSION");
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}
