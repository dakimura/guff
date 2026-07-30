//! Warm disk cache for the native package lister (PERF_TASKS_V2 §C-3c).
//!
//! Mirrors the `go list` stdout cache under `$GUFF_CACHE/golist/`: once the
//! package graph is computed, subsequent warm runs reload it instead of
//! re-walking GOMODCACHE + GOROOT (~0.8s → tens of ms).
//!
//! Reads are allowed under `--no-cache` (C-7 golist peek pattern); writes stay
//! gated by [`cache_enabled`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::package::{DriverResponse, Module, Package};
use crate::typecheck::TypecheckEnv;

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

fn cache_path(key: &str) -> Option<PathBuf> {
    let dir = guff_cache_dir()?;
    let prefix = key.get(..2).unwrap_or("00");
    Some(
        dir.join("native_list")
            .join(prefix)
            .join(format!("{key}.json")),
    )
}

const NATIVE_LIST_CACHE_VERSION: &str = "native-list-v2";

#[derive(Debug, Serialize, Deserialize)]
struct CachedGraph {
    version: String,
    compiler: String,
    arch: String,
    roots: Vec<String>,
    packages: Vec<CachedPackage>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedPackage {
    id: String,
    name: String,
    pkg_path: String,
    dir: PathBuf,
    go_files: Vec<PathBuf>,
    compiled_go_files: Vec<PathBuf>,
    ignored_files: Vec<PathBuf>,
    /// (source import path, resolved package id)
    imports: Vec<(String, String)>,
    deps: Vec<String>,
    module: Option<CachedModule>,
    for_test: String,
    #[serde(default)]
    has_cgo: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedModule {
    path: String,
    version: String,
    main: bool,
    indirect: bool,
    dir: PathBuf,
    go_mod: PathBuf,
    go_version: String,
}

pub(crate) fn cache_enabled(cfg: &Config) -> bool {
    if cfg.disable_cache {
        return false;
    }
    peek_allowed()
}

/// Read path remains open under `--no-cache` (C-7 golist peek pattern).
/// Writes stay gated by [`cache_enabled`].
fn peek_allowed() -> bool {
    for key in ["GUFF_CACHE", "GOLANGCI_LINT_CACHE"] {
        if let Ok(v) = std::env::var(key) {
            if v == "off" {
                return false;
            }
        }
    }
    true
}

pub(crate) fn try_load(cfg: &Config, patterns: &[String]) -> Option<DriverResponse> {
    if !peek_allowed() {
        return None;
    }
    // Avoid the `.go` filename walk when there is nothing to peek (same as
    // golist `try_peek_golist_cache`).
    let dir = guff_cache_dir()?;
    if !dir.join("native_list").is_dir() {
        return None;
    }
    let key = cache_key(cfg, patterns);
    let path = cache_path(&key)?;
    let bytes = std::fs::read(&path).ok()?;
    let cached: CachedGraph = serde_json::from_slice(&bytes).ok()?;
    if cached.version != NATIVE_LIST_CACHE_VERSION {
        return None;
    }
    // Invalidation is the cache key (go.mod / .go name set / env). No per-file
    // existence walk — that would dominate the warm hit path.
    Some(cached.into_driver_response(cfg))
}

pub(crate) fn store(cfg: &Config, patterns: &[String], response: &DriverResponse) {
    if !cache_enabled(cfg) {
        return;
    }
    let key = cache_key(cfg, patterns);
    let Some(path) = cache_path(&key) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let cached = CachedGraph::from_driver_response(response);
    if let Ok(bytes) = serde_json::to_vec(&cached) {
        let _ = std::fs::write(path, bytes);
    }
}

fn cache_key(cfg: &Config, patterns: &[String]) -> String {
    let mut h = Sha256::new();
    h.update(NATIVE_LIST_CACHE_VERSION.as_bytes());
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

    let mod_dir = find_go_mod_dir(&cfg.dir);
    if let Some(mod_dir) = mod_dir.as_ref() {
        for name in ["go.mod", "go.sum", "go.work", "go.work.sum"] {
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
        // Vendor mode changes resolution without touching go.mod contents.
        let modules_txt = mod_dir.join("vendor").join("modules.txt");
        match std::fs::read(&modules_txt) {
            Ok(bytes) => {
                h.update(b"file=vendor/modules.txt\n");
                h.update(&bytes);
                h.update(b"\n");
            }
            Err(_) => {
                h.update(b"file=vendor/modules.txt=missing\n");
            }
        }
    }

    let base = if cfg.dir.as_os_str().is_empty() {
        std::env::current_dir().unwrap_or_default()
    } else {
        cfg.dir.clone()
    };
    let module_path = mod_dir
        .as_ref()
        .and_then(|d| read_module_path(&d.join("go.mod")));
    hash_go_file_names(&mut h, &base, mod_dir.as_deref(), &pats, module_path.as_deref());

    let env = cfg.resolved_env();
    let mut interesting: Vec<(String, String)> = Vec::new();
    for entry in &env {
        if let Some((k, v)) = entry.split_once('=') {
            if matches!(
                k,
                "GOOS" | "GOARCH" | "CGO_ENABLED" | "GOTOOLCHAIN" | "GOROOT" | "GOFLAGS"
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

fn find_go_mod_dir(dir: &Path) -> Option<PathBuf> {
    let mut cur = if dir.as_os_str().is_empty() {
        std::env::current_dir().ok()?
    } else {
        dir.to_path_buf()
    };
    for _ in 0..64 {
        if cur.join("go.mod").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            break;
        }
    }
    None
}

fn read_module_path(go_mod: &Path) -> Option<String> {
    let text = std::fs::read_to_string(go_mod).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("module ") {
            let path = rest.split("//").next().unwrap_or(rest).trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

fn hash_go_file_names(
    h: &mut impl Digest,
    base: &Path,
    module_root: Option<&Path>,
    patterns: &[String],
    module_path: Option<&str>,
) {
    let mut files = Vec::new();
    for pat in patterns {
        collect_go_names(base, module_root, pat, module_path, &mut files);
    }
    files.sort();
    files.dedup();
    h.update(format!("go_files={}\n", files.len()).as_bytes());
    for f in files {
        h.update(f.as_bytes());
        h.update(b"\n");
    }
}

fn collect_go_names(
    base: &Path,
    module_root: Option<&Path>,
    pattern: &str,
    module_path: Option<&str>,
    out: &mut Vec<String>,
) {
    let walk_root = match pattern {
        "." | "" => base.to_path_buf(),
        "./..." | "..." => module_root.unwrap_or(base).to_path_buf(),
        p if p.ends_with("/...") => {
            let prefix = p.trim_end_matches("/...");
            resolve_pattern_dir(base, module_root, module_path, prefix)
                .unwrap_or_else(|| base.join(prefix.trim_start_matches("./")))
        }
        p => resolve_pattern_dir(base, module_root, module_path, p)
            .unwrap_or_else(|| base.join(p.trim_start_matches("./"))),
    };
    let recursive = pattern == "./..."
        || pattern == "..."
        || pattern.ends_with("/...");
    walk_go_names(&walk_root, module_root.unwrap_or(base), recursive, out);
}

fn resolve_pattern_dir(
    base: &Path,
    module_root: Option<&Path>,
    module_path: Option<&str>,
    pattern: &str,
) -> Option<PathBuf> {
    if pattern == "." || pattern.is_empty() {
        return Some(base.to_path_buf());
    }
    let p = Path::new(pattern);
    if pattern.starts_with('.') || p.is_absolute() {
        return Some(if p.is_absolute() {
            p.to_path_buf()
        } else {
            base.join(p)
        });
    }
    let (root, mpath) = (module_root?, module_path?);
    if pattern == mpath || pattern.starts_with(&format!("{mpath}/")) {
        let rel = pattern.trim_start_matches(mpath).trim_start_matches('/');
        return Some(if rel.is_empty() {
            root.to_path_buf()
        } else {
            root.join(rel)
        });
    }
    None
}

fn walk_go_names(dir: &Path, module_root: &Path, recursive: bool, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if !recursive {
                continue;
            }
            if matches!(
                name.as_ref(),
                "vendor" | "testdata" | "node_modules"
            ) || name.starts_with('.')
            {
                continue;
            }
            if path != module_root && path.join("go.mod").is_file() {
                continue;
            }
            walk_go_names(&path, module_root, true, out);
        } else if name.ends_with(".go") {
            let rel = path
                .strip_prefix(module_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push(rel);
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl CachedGraph {
    fn from_driver_response(response: &DriverResponse) -> Self {
        Self {
            version: NATIVE_LIST_CACHE_VERSION.to_string(),
            compiler: response.compiler.clone(),
            arch: response.arch.clone(),
            roots: response.roots.clone(),
            packages: response
                .packages
                .iter()
                .map(|p| CachedPackage {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    pkg_path: p.pkg_path.clone(),
                    dir: p.dir.clone(),
                    go_files: p.go_files.clone(),
                    compiled_go_files: p.compiled_go_files.clone(),
                    ignored_files: p.ignored_files.clone(),
                    imports: {
                        let mut v: Vec<_> = p
                            .imports
                            .iter()
                            .map(|(src, pkg)| (src.clone(), pkg.id.clone()))
                            .collect();
                        v.sort_by(|a, b| a.0.cmp(&b.0));
                        v
                    },
                    deps: p.deps.clone(),
                    module: p.module.as_ref().map(|m| CachedModule {
                        path: m.path.clone(),
                        version: m.version.clone(),
                        main: m.main,
                        indirect: m.indirect,
                        dir: m.dir.clone(),
                        go_mod: m.go_mod.clone(),
                        go_version: m.go_version.clone(),
                    }),
                    for_test: p.for_test.clone(),
                    has_cgo: p.has_cgo,
                })
                .collect(),
        }
    }

    fn into_driver_response(self, cfg: &Config) -> DriverResponse {
        let env = cfg.resolved_env();
        let arch = if self.arch.is_empty() {
            TypecheckEnv::from_env(&env, "gc").arch
        } else {
            self.arch
        };
        DriverResponse {
            compiler: if self.compiler.is_empty() {
                "gc".into()
            } else {
                self.compiler
            },
            arch,
            roots: self.roots,
            packages: self
                .packages
                .into_iter()
                .map(|p| Arc::new(p.into_package()))
                .collect(),
            ..DriverResponse::default()
        }
    }
}

impl CachedPackage {
    fn into_package(self) -> Package {
        let mut imports = std::collections::HashMap::new();
        for (src, id) in &self.imports {
            imports.insert(
                src.clone(),
                Arc::new(Package {
                    id: id.clone(),
                    pkg_path: id.clone(),
                    ..Package::default()
                }),
            );
        }
        Package {
            id: self.id,
            name: self.name,
            pkg_path: self.pkg_path,
            dir: self.dir,
            go_files: self.go_files,
            compiled_go_files: self.compiled_go_files,
            ignored_files: self.ignored_files,
            imports,
            deps: self.deps,
            module: self.module.map(|m| Module {
                path: m.path,
                version: m.version,
                replace: None,
                main: m.main,
                indirect: m.indirect,
                dir: m.dir,
                go_mod: m.go_mod,
                go_version: m.go_version,
                error: None,
            }),
            for_test: self.for_test,
            has_cgo: self.has_cgo,
            ..Package::default()
        }
    }
}
