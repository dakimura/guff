//! GOMODCACHE discovery and `module@version` directory layout.

use std::env;
use std::path::{Path, PathBuf};

use crate::escape::escape_path;

/// Resolved module cache root (`GOMODCACHE`).
#[derive(Debug, Clone)]
pub struct ModCache {
    pub root: PathBuf,
}

impl ModCache {
    pub fn from_env() -> Self {
        Self {
            root: default_gomodcache(),
        }
    }

    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
        }
    }
}

/// Directory containing the extracted sources for `module_path`@`version`.
pub fn module_dir(cache: &Path, module_path: &str, version: &str) -> Option<PathBuf> {
    let escaped = escape_path(module_path)?;
    Some(cache.join(format!("{escaped}@{version}")))
}

/// Default `GOMODCACHE` without shelling out to `go env`.
///
/// Order: `$GOMODCACHE` → `$GOPATH/pkg/mod` → `$HOME/go/pkg/mod`.
pub fn default_gomodcache() -> PathBuf {
    if let Ok(v) = env::var("GOMODCACHE") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    if let Ok(gopath) = env::var("GOPATH") {
        let first = gopath
            .split(if cfg!(windows) { ';' } else { ':' })
            .find(|p| !p.is_empty());
        if let Some(p) = first {
            return PathBuf::from(p).join("pkg").join("mod");
        }
    }
    env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("go").join("pkg").join("mod"))
        .unwrap_or_else(|| PathBuf::from("go/pkg/mod"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_dir_joins_escaped_path() {
        let dir = module_dir(Path::new("/mod"), "github.com/Foo/bar", "v1.2.3").unwrap();
        assert_eq!(
            dir,
            PathBuf::from("/mod/github.com/!foo/bar@v1.2.3")
        );
    }
}
