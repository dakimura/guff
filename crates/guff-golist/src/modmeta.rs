//! Permanent per-package metadata cache for immutable modules (C-3c §④).
//!
//! GOMODCACHE packages are content-addressed (`module@version`) and GOROOT
//! packages are fixed per toolchain — once scanned for a given GOOS/GOARCH /
//! build-tags fingerprint they never need to be re-read. The main module is
//! never cached here (it changes).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use guff_build::{Context, Package as BuildPackage};

const MODMETA_VERSION: &str = "modmeta-v1";

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

/// Cache key material that must match for a hit to be valid.
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

fn key_hash(key: &ModMetaKey<'_>) -> String {
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
    h.update(key.pkg_path.as_bytes());
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
    let root = cache_root()?;
    let hash = key_hash(key);
    let prefix = hash.get(..2).unwrap_or("00");
    Some(root.join(prefix).join(format!("{hash}.json")))
}

/// Load cached package metadata, or `None` on miss / disabled.
pub fn load(key: &ModMetaKey<'_>) -> Option<CachedPkgMeta> {
    if !cache_enabled() {
        return None;
    }
    // Immutable packages without a version cannot be keyed safely.
    if !key.standard && key.module_version.is_empty() {
        return None;
    }
    let path = cache_path(key)?;
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Store package metadata (best-effort).
pub fn store(key: &ModMetaKey<'_>, meta: &CachedPkgMeta) {
    if !cache_enabled() {
        return;
    }
    if !key.standard && key.module_version.is_empty() {
        return;
    }
    let Some(path) = cache_path(key) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec(meta) {
        let _ = std::fs::write(path, bytes);
    }
}

/// Read GOROOT/VERSION (or fallback string) for stdlib cache keys.
pub fn goroot_version(goroot: &Path) -> String {
    let path = goroot.join("VERSION");
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

/// Import a package directory, using the modmeta cache for immutable modules.
pub fn import_dir_cached(
    ctxt: &Context,
    dir: &Path,
    key: &ModMetaKey<'_>,
) -> Result<BuildPackage, guff_build::BuildError> {
    if let Some(meta) = load(key) {
        let mut pkg = BuildPackage {
            dir: dir.to_path_buf(),
            import_path: key.pkg_path.to_string(),
            goroot: key.standard,
            ..BuildPackage::default()
        };
        meta.apply_to(&mut pkg);
        return Ok(pkg);
    }
    let pkg = ctxt.import_dir(dir)?;
    // Only persist successful scans of immutable packages.
    if key.standard || !key.module_version.is_empty() {
        store(key, &CachedPkgMeta::from_build(&pkg));
    }
    Ok(pkg)
}
