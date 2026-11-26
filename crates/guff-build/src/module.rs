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
    /// `replace` directives (old path[/version] → new path[/version or local]).
    pub replaces: Vec<Replace>,
    /// True when the file contains an `exclude` directive (native lister bails).
    pub has_exclude: bool,
    /// True when the file contains a `retract` directive (native lister bails).
    pub has_retract: bool,
}

/// A `require` directive entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Require {
    pub path: String,
    pub version: String,
    pub indirect: bool,
}

/// A `replace` directive entry.
///
/// `new_version` empty means a local filesystem replace (`=> ../foo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replace {
    pub old_path: String,
    pub old_version: String,
    pub new_path: String,
    pub new_version: String,
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
    let mut replaces = Vec::new();
    let mut has_exclude = false;
    let mut has_retract = false;
    let mut block: Option<BlockKind> = None;

    for raw in data.lines() {
        let indirect_hint = raw.contains("//") && raw.contains("indirect");
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line == ")" {
            block = None;
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
        if line.starts_with("toolchain ") {
            continue;
        }
        if starts_directive(line, "exclude") {
            has_exclude = true;
            if line.contains('(') && !line.contains(')') {
                block = Some(BlockKind::Ignore);
            }
            continue;
        }
        if starts_directive(line, "retract") {
            has_retract = true;
            if line.contains('(') && !line.contains(')') {
                block = Some(BlockKind::Ignore);
            }
            continue;
        }
        if starts_directive(line, "require") {
            if let Some(rest) = open_block(line, "require") {
                block = Some(BlockKind::Require);
                if !rest.is_empty() {
                    if let Some(mut req) = parse_require_line(rest) {
                        req.indirect = req.indirect || indirect_hint;
                        requires.push(req);
                    }
                }
            } else if let Some(mut req) =
                parse_require_line(line.strip_prefix("require ").unwrap_or(line))
            {
                req.indirect = req.indirect || indirect_hint;
                requires.push(req);
            }
            continue;
        }
        if starts_directive(line, "replace") {
            if let Some(rest) = open_block(line, "replace") {
                block = Some(BlockKind::Replace);
                if !rest.is_empty() {
                    if let Some(r) = parse_replace_line(rest) {
                        replaces.push(r);
                    }
                }
            } else if let Some(r) = parse_replace_line(line.strip_prefix("replace ").unwrap_or(line))
            {
                replaces.push(r);
            }
            continue;
        }
        match block {
            Some(BlockKind::Require) => {
                if let Some(mut req) = parse_require_line(line) {
                    req.indirect = req.indirect || indirect_hint;
                    requires.push(req);
                }
            }
            Some(BlockKind::Replace) => {
                if let Some(r) = parse_replace_line(line) {
                    replaces.push(r);
                }
            }
            Some(BlockKind::Ignore) => {}
            None => {}
        }
    }

    if module_path.is_empty() {
        return Err(BuildError::Import("go.mod: missing module directive".into()));
    }

    Ok(ModFile {
        module_path,
        go_version,
        requires,
        replaces,
        has_exclude,
        has_retract,
    })
}

#[derive(Clone, Copy)]
enum BlockKind {
    Require,
    Replace,
    Ignore,
}

fn starts_directive(line: &str, name: &str) -> bool {
    line == name
        || line.starts_with(&format!("{name} "))
        || line.starts_with(&format!("{name}("))
}

/// Returns `Some(rest)` when `line` opens a parenthesized block (`name (`).
fn open_block<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(name)?.trim_start();
    let rest = rest.strip_prefix('(')?.trim();
    if rest == ")" {
        return Some("");
    }
    Some(rest)
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
    let indirect = line.contains("indirect");
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
        indirect,
    })
}

fn parse_replace_line(line: &str) -> Option<Replace> {
    let line = line.trim().trim_end_matches(')');
    if line.is_empty() {
        return None;
    }
    let (left, right) = line.split_once("=>")?;
    let mut left = left.split_whitespace();
    let old_path = left.next()?.to_string();
    let old_version = left.next().unwrap_or("").to_string();
    let mut right = right.split_whitespace();
    let new_path = right.next()?.to_string();
    let new_version = right.next().unwrap_or("").to_string();
    Some(Replace {
        old_path,
        old_version,
        new_path,
        new_version,
    })
}

fn strip_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}
