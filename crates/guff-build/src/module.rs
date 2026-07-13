//! `go.mod` parsing and module root discovery.

use std::fs;
use std::path::{Path, PathBuf};

use crate::package::BuildError;

/// Parsed contents of a `go.mod` file (subset).
///
/// Equivalent to the fields `go/build` reads via `go list` in module mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModFile {
    pub module_path: String,
    pub go_version: Option<String>,
    pub requires: Vec<Require>,
}

/// A `require` directive entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Require {
    pub path: String,
    pub version: String,
}

/// Walks upward from `start` to find a directory containing `go.mod`.
pub fn find_module_root(start: &Path) -> Option<PathBuf> {
    let start = start.canonicalize().ok()?;
    let mut dir = start;
    loop {
        if dir.join("go.mod").is_file() {
            return Some(dir.to_path_buf());
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Parses a `go.mod` file at `path`.
pub fn parse_mod_file(path: &Path) -> Result<ModFile, BuildError> {
    let data = fs::read_to_string(path)?;
    parse_mod_contents(&data)
}

/// Parses `go.mod` contents.
pub fn parse_mod_contents(data: &str) -> Result<ModFile, BuildError> {
    let mut module_path = String::new();
    let mut go_version = None;
    let mut requires = Vec::new();
    let mut in_require_block = false;

    for line in data.lines() {
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        if line == ")" {
            in_require_block = false;
            continue;
        }
        if line.starts_with("module ") {
            module_path = line["module ".len()..].trim().to_string();
            continue;
        }
        if line.starts_with("go ") {
            go_version = Some(line["go ".len()..].trim().to_string());
            continue;
        }
        if line.starts_with("require (") {
            in_require_block = true;
            let rest = line["require (".len()..].trim();
            if !rest.is_empty() && rest != ")" {
                if let Some(req) = parse_require_line(rest) {
                    requires.push(req);
                }
            }
            continue;
        }
        if line.starts_with("require ") || in_require_block {
            let text = line.strip_prefix("require ").unwrap_or(line);
            if let Some(req) = parse_require_line(text) {
                requires.push(req);
            }
        }
    }

    if module_path.is_empty() {
        return Err(BuildError::Import("go.mod: missing module directive".into()));
    }

    Ok(ModFile {
        module_path,
        go_version,
        requires,
    })
}

/// Maps `import_path` within a module to a filesystem directory.
pub fn module_import_dir(module_root: &Path, module_path: &str, import_path: &str) -> Option<PathBuf> {
    if import_path == module_path {
        return Some(module_root.to_path_buf());
    }
    let prefix = format!("{module_path}/");
    if !import_path.starts_with(&prefix) {
        return None;
    }
    let rel = &import_path[prefix.len()..];
    if rel.is_empty() || rel.contains("..") {
        return None;
    }
    Some(module_root.join(rel))
}

fn parse_require_line(line: &str) -> Option<Require> {
    let line = line.trim().trim_end_matches(')');
    if line.is_empty() {
        return None;
    }
    let mut parts = line.split_whitespace();
    let path = parts.next()?;
    let version = parts.next().unwrap_or("").to_string();
    Some(Require {
        path: path.to_string(),
        version,
    })
}

fn strip_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}
