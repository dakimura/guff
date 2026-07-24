//! Path display helpers (golangci `output.path-mode` / `path-prefix`).

use std::path::{Component, Path, PathBuf};

/// golangci-lint `output.path-mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathMode {
    /// Paths relative to the process working directory (golangci default).
    #[default]
    Rel,
    /// Keep absolute paths (`output.path-mode: abs` / `--path-mode abs`).
    Abs,
}

impl PathMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "rel" | "relative" => Some(Self::Rel),
            "abs" | "absolute" => Some(Self::Abs),
            _ => None,
        }
    }
}

/// Rewrite `filename` for issue output.
///
/// - `PathMode::Rel`: strip the current working directory prefix when possible.
/// - `PathMode::Abs`: leave as-is (typically already absolute from FileSet).
/// - `path_prefix`: optional golangci `output.path-prefix` prepended after mode.
pub fn format_issue_path(filename: &str, mode: PathMode, path_prefix: Option<&str>) -> String {
    if filename.is_empty() {
        return String::new();
    }
    let mut path = match mode {
        PathMode::Abs => filename.replace('\\', "/"),
        PathMode::Rel => relativize_to_cwd(filename),
    };
    if let Some(prefix) = path_prefix {
        if !prefix.is_empty() {
            let p = prefix.replace('\\', "/");
            path = if p.ends_with('/') {
                format!("{p}{path}")
            } else {
                format!("{p}/{path}")
            };
        }
    }
    path
}

fn relativize_to_cwd(filename: &str) -> String {
    let norm = filename.replace('\\', "/");
    let path = Path::new(&norm);
    let Ok(cwd) = std::env::current_dir() else {
        return norm;
    };
    // Lexical strip first (FileSet paths are usually absolute under cwd).
    if let Ok(rel) = path.strip_prefix(&cwd) {
        let s = rel.to_string_lossy().replace('\\', "/");
        if !s.is_empty() {
            return s;
        }
    }
    // Resolve symlinks / `..` when both sides canonicalize.
    if let (Ok(abs), Ok(cwd_abs)) = (path.canonicalize(), cwd.canonicalize()) {
        if let Ok(rel) = abs.strip_prefix(&cwd_abs) {
            let s = rel.to_string_lossy().replace('\\', "/");
            if !s.is_empty() {
                return s;
            }
        }
    }
    // Already relative, or outside cwd — keep as normalized input.
    if path.is_relative() {
        return normalize_lexically(path)
            .to_string_lossy()
            .replace('\\', "/");
    }
    norm
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_mode_parse() {
        assert_eq!(PathMode::parse(""), Some(PathMode::Rel));
        assert_eq!(PathMode::parse("abs"), Some(PathMode::Abs));
        assert_eq!(PathMode::parse("ABS"), Some(PathMode::Abs));
        assert!(PathMode::parse("bogus").is_none());
    }

    #[test]
    fn format_abs_keeps_input() {
        let p = "/tmp/proj/a.go";
        assert_eq!(format_issue_path(p, PathMode::Abs, None), p);
    }

    #[test]
    fn format_prefix() {
        let p = format_issue_path("a.go", PathMode::Abs, Some("mod"));
        assert_eq!(p, "mod/a.go");
    }
}
