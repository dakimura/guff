//! [`Config`] for package loading.
//!
//! Port of `packages.Config` from `packages.go`.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::load_mode::LoadMode;

/// Configuration for [`super::load`].
///
/// Equivalent to `packages.Config`.
#[derive(Debug, Clone)]
pub struct Config {
    /// Fields to populate on loaded packages.
    pub mode: LoadMode,
    /// Working directory for the build-system query tool.
    pub dir: PathBuf,
    /// Environment for subprocesses. `None` uses the current process environment.
    pub env: Option<Vec<String>>,
    /// Flags passed through to `go list`.
    pub build_flags: Vec<String>,
    /// Include test packages and test-augmented variants.
    pub tests: bool,
    /// Absolute file path → unsaved contents (editor overlays).
    pub overlay: HashMap<PathBuf, Vec<u8>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: LoadMode::default(),
            dir: std::env::current_dir().unwrap_or_default(),
            env: None,
            build_flags: Vec::new(),
            tests: false,
            overlay: HashMap::new(),
        }
    }
}

impl Config {
    /// Effective load mode after zero-value normalization and implied flags.
    pub fn effective_mode(&self) -> LoadMode {
        self.mode.normalize().implied()
    }

    /// Environment variables for subprocess invocation.
    pub fn resolved_env(&self) -> Vec<String> {
        match &self.env {
            Some(env) => env.clone(),
            None => std::env::vars().map(|(k, v)| format!("{k}={v}")).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_mode::LoadMode;

    #[test]
    fn default_mode_is_load_files_after_normalize() {
        let cfg = Config::default();
        assert_eq!(cfg.mode.normalize(), LoadMode::LOAD_FILES);
    }

    #[test]
    fn effective_mode_adds_implied_flags() {
        let cfg = Config {
            mode: LoadMode::NEED_TYPES,
            ..Config::default()
        };
        assert!(cfg.effective_mode().contains(LoadMode::NEED_IMPORTS));
    }
}
