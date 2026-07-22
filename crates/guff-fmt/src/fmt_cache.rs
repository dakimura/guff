//! Persistent format-check cache (PERF_TASKS Task 1 warm path).
//!
//! Stores per-file results of `format(src) == src` (plus first differing lines)
//! under `${GUFF_CACHE}/fmt_check/v1/`, keyed by
//! `sha256(formatter \\0 options_fp \\0 content_sha256)`.
//!
//! On a warm `GUFF_CACHE`, check mode skips `gofumpt -l` / `gci list` / …
//! entirely when every file hits. Findings stay byte-identical to a cold miss
//! path because values are the lines we already computed via `check_file`.

use std::fs;
use std::io;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

/// Schema directory name — bump when the on-disk record format changes.
pub const FMT_CHECK_SCHEMA: &str = "v1";

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

/// One file's cached check result for a single formatter + options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CachedCheck {
    /// `format(src) == src` — no finding.
    Clean,
    /// 1-based lines of the first differing hunk(s), same as [`crate::runner::first_changed_lines`].
    Lines(Vec<i64>),
}

/// Disk-backed format-check cache.
#[derive(Debug, Clone)]
pub struct FormatCheckCache {
    root: PathBuf,
}

impl FormatCheckCache {
    /// `${cache_dir}/fmt_check/v1`. Creates the directory if needed.
    pub fn open(cache_dir: impl Into<PathBuf>) -> io::Result<Self> {
        let root = cache_dir.into().join("fmt_check").join(FMT_CHECK_SCHEMA);
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Look up a prior result. Corrupted / unreadable entries are treated as miss.
    pub fn get(
        &self,
        formatter: &str,
        options_fp: &str,
        content_hash: &str,
    ) -> Option<CachedCheck> {
        let path = self.entry_path(formatter, options_fp, content_hash);
        let bytes = fs::read(&path).ok()?;
        parse_record(&bytes)
    }

    /// Store a result. Failures are ignored (cache is best-effort).
    pub fn put(
        &self,
        formatter: &str,
        options_fp: &str,
        content_hash: &str,
        value: &CachedCheck,
    ) {
        let path = self.entry_path(formatter, options_fp, content_hash);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&path, encode_record(value));
    }

    fn entry_path(&self, formatter: &str, options_fp: &str, content_hash: &str) -> PathBuf {
        let mut h = Sha256::new();
        h.update(formatter.as_bytes());
        h.update([0]);
        h.update(options_fp.as_bytes());
        h.update([0]);
        h.update(content_hash.as_bytes());
        let dig = hex_encode(h.finalize());
        // Two-level fanout to keep directories shallow.
        self.root.join(&dig[..2]).join(&dig[2..])
    }
}

/// SHA-256 hex of raw file bytes (cache content key).
pub fn content_hash(src: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(src);
    hex_encode(h.finalize())
}

fn encode_record(v: &CachedCheck) -> Vec<u8> {
    match v {
        CachedCheck::Clean => b"OK\n".to_vec(),
        CachedCheck::Lines(lines) => {
            let mut out = Vec::from(&b"LINES\n"[..]);
            for line in lines {
                out.extend_from_slice(line.to_string().as_bytes());
                out.push(b'\n');
            }
            out
        }
    }
}

fn parse_record(bytes: &[u8]) -> Option<CachedCheck> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    match lines.next()? {
        "OK" => Some(CachedCheck::Clean),
        "LINES" => {
            let mut v = Vec::new();
            for l in lines {
                if l.is_empty() {
                    continue;
                }
                v.push(l.parse().ok()?);
            }
            Some(CachedCheck::Lines(v))
        }
        _ => None,
    }
}

/// Resolve `${GUFF_CACHE}` (same precedence as issue/seed caches). `off` / empty → None.
pub fn format_cache_dir_from_env() -> Option<PathBuf> {
    for key in ["GUFF_CACHE", "GOLANGCI_LINT_CACHE"] {
        if let Ok(v) = std::env::var(key) {
            let t = v.trim();
            if t.is_empty() {
                continue;
            }
            if t.eq_ignore_ascii_case("off") {
                return None;
            }
            return Some(PathBuf::from(t));
        }
    }
    None
}

/// Stable fingerprint of formatter settings (must change when behavior changes).
pub fn fingerprint_parts(parts: &[(&str, &str)]) -> String {
    let mut h = Sha256::new();
    for (k, v) in parts {
        h.update(k.as_bytes());
        h.update([0]);
        h.update(v.as_bytes());
        h.update([0]);
    }
    hex_encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_clean_and_lines() {
        let tmp = tempdir().unwrap();
        let cache = FormatCheckCache::open(tmp.path()).unwrap();
        let ch = content_hash(b"package p\n");
        cache.put("gofumpt", "fp1", &ch, &CachedCheck::Clean);
        assert_eq!(
            cache.get("gofumpt", "fp1", &ch),
            Some(CachedCheck::Clean)
        );
        cache.put(
            "gofumpt",
            "fp1",
            &ch,
            &CachedCheck::Lines(vec![3, 7]),
        );
        assert_eq!(
            cache.get("gofumpt", "fp1", &ch),
            Some(CachedCheck::Lines(vec![3, 7]))
        );
        // Different options → miss
        assert!(cache.get("gofumpt", "fp2", &ch).is_none());
    }
}
