//! On-disk persistence for per-package source-seed overlays (PERF Task 4).
//!
//! Each source dependency's [`WorkerOverlays`] is stored under
//! `${GUFF_CACHE}/seed/`, keyed by the package's content `self_hash` **and** a
//! fingerprint of the seed prefix it was type-checked against (`base_fp`).
//! Matching both is required: overlay ids that point into the shared base are
//! absolute, and [`guff_types::ExportSeed::merge_wave`]'s remapper only shifts
//! overlay-local ids.
//!
//! Schema / corrupt mismatches are treated as cache misses (silent rebuild).

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use guff_types::{WorkerOverlays, SEED_OVERLAY_SCHEMA};

use crate::package::Package;

/// Env var: set to `0`/`false`/`off` to disable seed persistence. Unset or any
/// other value leaves it **enabled** (default ON).
///
/// Persistence is cheap on a miss: source is read once (shared with the parser),
/// hashed on the worker threads, and the disk writes run on a background thread
/// off the critical path — so a fresh/empty cache pays no measurable penalty,
/// while a persistent `GUFF_CACHE` gets the seed-hot speedup on the next run.
pub const ENV_GUFF_SEED_PERSIST: &str = "GUFF_SEED_PERSIST";

/// Whether per-package seed overlay persistence is enabled (default ON).
pub fn seed_persist_enabled() -> bool {
    match env::var(ENV_GUFF_SEED_PERSIST) {
        Ok(v) => {
            let v = v.trim();
            // Explicit opt-out only; every other value (including unrecognized
            // ones) keeps the default-on behavior.
            !(v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off"))
        }
        // Unset → enabled (default ON).
        Err(_) => true,
    }
}

/// Resolve `${GUFF_CACHE}/seed` (same precedence as the issue cache dir).
///
/// Returns `None` when the cache is explicitly disabled (`GUFF_CACHE=off`) or
/// no directory can be resolved.
pub fn seed_cache_dir() -> Option<PathBuf> {
    for key in ["GUFF_CACHE", "GOLANGCI_LINT_CACHE"] {
        if let Ok(v) = env::var(key) {
            if v.is_empty() {
                continue;
            }
            if v == "off" {
                return None;
            }
            let p = PathBuf::from(&v);
            if !p.is_absolute() {
                return None;
            }
            return Some(p.join("seed"));
        }
    }
    user_cache_dir().map(|base| base.join("guff").join("seed"))
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
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        None
    }
}

/// Content hash of a package's own `compiled_go_files` (sorted), matching
/// `IssueCache::self_hash` in guff-runner so keys stay aligned with dep-hash.
///
/// Returns `None` if any file cannot be read. Prefer
/// [`pkg_self_hash_from_sources`] on the seed path, where the source bytes have
/// already been read for parsing — that avoids reading every dependency's
/// source a second time just to key the cache.
pub fn pkg_self_hash(pkg: &Package) -> Option<String> {
    let mut sources = Vec::with_capacity(pkg.compiled_go_files.len());
    for f in &pkg.compiled_go_files {
        sources.push((f.clone(), fs::read(f).ok()?));
    }
    Some(pkg_self_hash_from_sources(&pkg.pkg_path, &sources))
}

/// Content hash from source bytes already read (see [`pkg_self_hash`]).
///
/// Byte-identical to [`pkg_self_hash`] for the same package: files are hashed
/// in sorted-path order regardless of the order in `sources`, so the caller may
/// pass them in `compiled_go_files` (parse) order.
pub fn pkg_self_hash_from_sources(pkg_path: &str, sources: &[(PathBuf, Vec<u8>)]) -> String {
    let mut files: Vec<&(PathBuf, Vec<u8>)> = sources.iter().collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = Sha256::new();
    h.update(b"package hash\n");
    h.update(format!("pkgpath {pkg_path}\n").as_bytes());
    for (f, bytes) in &files {
        let mut fh = Sha256::new();
        fh.update(bytes);
        let dig = hex_encode(&fh.finalize());
        let display = f.to_string_lossy();
        h.update(format!("file {display} {dig}\n").as_bytes());
    }
    hex_encode(&h.finalize())
}

/// Inputs that identify the seed prefix a worker overlay was built against.
#[derive(Debug, Clone)]
pub struct BaseFingerprintInput<'a> {
    pub go_version: &'a str,
    pub arch: &'a str,
    /// `(types, objects, scopes, packages)` lengths of the frozen seed at
    /// Phase A (export S0) — held constant for the whole build; source merges
    /// grow the live seed but S0 lens stay in the fingerprint via this field
    /// plus the ordered `merged` list.
    pub s0_lens: (usize, usize, usize, usize),
    /// Sorted import-cache keys immediately after Phase A.
    pub s0_import_paths: &'a [String],
    /// Source packages already merged, in merge order (wave order × path sort).
    pub merged: &'a [(String, String)], // (path, self_hash)
}

/// Deterministic fingerprint of the seed prefix at a wave boundary.
///
/// Prefer [`base_fingerprint_extend`] across waves so each step is O(1) in the
/// number of previously merged packages rather than O(n) SHA over the full list.
pub fn base_fingerprint(input: &BaseFingerprintInput<'_>) -> String {
    let mut h = Sha256::new();
    h.update(format!("seed-base v{SEED_OVERLAY_SCHEMA}\n").as_bytes());
    h.update(b"fp-chain v2\n");
    h.update(format!("go_version {}\n", input.go_version).as_bytes());
    h.update(format!("arch {}\n", input.arch).as_bytes());
    let (ty, ob, sc, pk) = input.s0_lens;
    h.update(format!("s0_lens {ty} {ob} {sc} {pk}\n").as_bytes());
    for path in input.s0_import_paths {
        h.update(format!("s0_import {path}\n").as_bytes());
    }
    for (path, self_hash) in input.merged {
        h.update(format!("merged {path} {self_hash}\n").as_bytes());
    }
    hex_encode(&h.finalize())
}

/// O(1) extension of a prior [`base_fingerprint`] after one more merged package.
pub fn base_fingerprint_extend(prev: &str, path: &str, self_hash: &str) -> String {
    let mut h = Sha256::new();
    h.update(prev.as_bytes());
    h.update(b"\n");
    h.update(format!("merged {path} {self_hash}\n").as_bytes());
    hex_encode(&h.finalize())
}

/// On-disk path for one overlay blob.
///
/// The import `path` is part of the key: multiple `go list` package ids can
/// share the same `pkg_path` + file set (identical `self_hash`) and the same
/// wave `base_fp`; without `path` they would clobber one file and cache hits
/// would load the wrong overlay (silent findings loss).
pub fn overlay_path(seed_dir: &Path, path: &str, self_hash: &str, base_fp: &str) -> PathBuf {
    let mut ph = Sha256::new();
    ph.update(path.as_bytes());
    let path_key = hex_encode(&ph.finalize());
    seed_dir.join(format!(
        "{path_key}.{self_hash}.{base_fp}.v{SEED_OVERLAY_SCHEMA}.bin"
    ))
}

/// Load a persisted overlay. Returns `None` on miss, schema mismatch, or
/// corruption (caller rebuilds from source).
pub fn load_overlay(
    seed_dir: &Path,
    path: &str,
    self_hash: &str,
    base_fp: &str,
) -> Option<WorkerOverlays> {
    let file = overlay_path(seed_dir, path, self_hash, base_fp);
    let bytes = fs::read(&file).ok()?;
    WorkerOverlays::decode(&bytes).ok()
}

/// Persist an overlay synchronously (caller must have already cleared
/// FileSet-absolute positions via [`WorkerOverlays::clear_source_positions`]).
/// Failures are silent — the cache is best-effort. On the hot seed path prefer
/// encoding on the worker and handing the bytes to an [`OverlayWriter`] so the
/// disk syscalls stay off the critical path.
pub fn save_overlay(
    seed_dir: &Path,
    path: &str,
    self_hash: &str,
    base_fp: &str,
    overlay: &WorkerOverlays,
) {
    let Ok(bytes) = overlay.encode() else {
        return;
    };
    write_overlay_bytes(&overlay_path(seed_dir, path, self_hash, base_fp), &bytes);
}

/// Atomically write pre-encoded overlay bytes to `file` (temp file + rename).
/// Best-effort: any I/O error leaves no partial file and is otherwise ignored.
pub fn write_overlay_bytes(file: &Path, bytes: &[u8]) {
    if let Some(parent) = file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = file.with_extension("bin.tmp");
    if fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(bytes))
        .is_err()
    {
        let _ = fs::remove_file(&tmp);
        return;
    }
    if fs::rename(&tmp, file).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

/// Background thread that performs overlay disk writes off the type-check
/// critical path. Workers encode overlays (cheap, parallel) and [`submit`] the
/// resulting bytes; the writer drains them while later waves compute. Call
/// [`finish`] once to flush the tail before relying on the cache.
///
/// [`submit`]: OverlayWriter::submit
/// [`finish`]: OverlayWriter::finish
pub struct OverlayWriter {
    tx: Option<std::sync::mpsc::Sender<(PathBuf, Vec<u8>)>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl OverlayWriter {
    /// Spawn the writer thread. If the thread cannot be spawned, [`submit`]
    /// becomes a no-op (bytes are dropped rather than buffered unboundedly).
    ///
    /// [`submit`]: OverlayWriter::submit
    pub fn spawn() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<(PathBuf, Vec<u8>)>();
        let handle = std::thread::Builder::new()
            .name("guff-seed-writer".into())
            .spawn(move || {
                for (file, bytes) in rx {
                    write_overlay_bytes(&file, &bytes);
                }
            })
            .ok();
        // If the spawn failed, drop the sender so `submit` short-circuits
        // instead of buffering messages that no thread will ever consume.
        let tx = handle.as_ref().map(|_| tx);
        Self { tx, handle }
    }

    /// Queue one overlay for writing. Non-blocking; drops the bytes if the
    /// writer thread is gone.
    pub fn submit(&self, file: PathBuf, bytes: Vec<u8>) {
        if let Some(tx) = &self.tx {
            let _ = tx.send((file, bytes));
        }
    }

    /// Close the queue and wait for all pending writes to land.
    pub fn finish(mut self) {
        drop(self.tx.take());
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for OverlayWriter {
    fn drop(&mut self) {
        // Safety net if `finish` was not called: close and join so writes are
        // not silently abandoned (and the thread is not detached).
        drop(self.tx.take());
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_fingerprint_is_order_sensitive_for_merged() {
        let s0_paths = vec!["fmt".into(), "io".into()];
        let a = base_fingerprint(&BaseFingerprintInput {
            go_version: "go1.26",
            arch: "arm64",
            s0_lens: (1, 2, 3, 4),
            s0_import_paths: &s0_paths,
            merged: &[("a".into(), "h1".into()), ("b".into(), "h2".into())],
        });
        let b = base_fingerprint(&BaseFingerprintInput {
            go_version: "go1.26",
            arch: "arm64",
            s0_lens: (1, 2, 3, 4),
            s0_import_paths: &s0_paths,
            merged: &[("b".into(), "h2".into()), ("a".into(), "h1".into())],
        });
        assert_ne!(a, b);
    }

    #[test]
    fn base_fingerprint_stable_for_identical_inputs() {
        let s0_paths = vec!["fmt".into()];
        let input = BaseFingerprintInput {
            go_version: "go1.26",
            arch: "arm64",
            s0_lens: (10, 20, 30, 40),
            s0_import_paths: &s0_paths,
            merged: &[("x".into(), "abc".into())],
        };
        assert_eq!(base_fingerprint(&input), base_fingerprint(&input));
    }

    #[test]
    fn seed_persist_enabled_respects_flag() {
        fn parse(v: Option<&str>) -> bool {
            match v {
                Some(v) => {
                    let v = v.trim();
                    v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("on")
                }
                None => false,
            }
        }
        assert!(!parse(None));
        assert!(parse(Some("1")));
        assert!(parse(Some("on")));
        assert!(!parse(Some("0")));
        assert!(!parse(Some("false")));
    }

    #[test]
    fn load_overlay_miss_on_corrupt() {
        let dir = std::env::temp_dir().join(format!(
            "guff-seed-test-{}",
            std::process::id()
        ));
        let _ = fs::create_dir_all(&dir);
        let path = overlay_path(&dir, "example.com/pkg", "abc", "0123456789abcdef");
        fs::write(&path, b"not-valid-bincode").unwrap();
        assert!(load_overlay(&dir, "example.com/pkg", "abc", "0123456789abcdef").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn overlay_path_differs_by_import_path() {
        let dir = Path::new("/tmp/seed");
        let a = overlay_path(dir, "example.com/a", "samehash", "samefp");
        let b = overlay_path(dir, "example.com/b", "samehash", "samefp");
        assert_ne!(a, b);
    }
}
