//! `go.work` parsing and workspace module set.
//!
//! A workspace lists local modules via `use` and optional workspace-level
//! `replace` directives. Every `use` module is a main module for import
//! resolution (Go 1.18+).

use std::fs;
use std::path::{Path, PathBuf};

use guff_build::{parse_mod_file, ModFile, Replace};

use crate::bail::{Bail, BailReason};

/// Parsed `go.work` + loaded `go.mod` for each `use` entry.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Directory containing `go.work` (or the sole module root when absent).
    pub root: PathBuf,
    pub go_version: Option<String>,
    pub modules: Vec<WorkspaceModule>,
    /// Workspace-level replaces (take precedence over module replaces).
    pub replaces: Vec<Replace>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceModule {
    pub dir: PathBuf,
    pub mod_file: ModFile,
}

/// Walks upward from `start` looking for `go.work`.
pub fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.canonicalize().ok()?;
    loop {
        if dir.join("go.work").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Builds a [`Workspace`]: multi-module if `go.work` is found, else a single
/// module from `module_root`.
pub fn load_workspace(src_dir: &Path, module_root: &Path) -> Result<Workspace, Bail> {
    if let Some(ws_root) = find_workspace_root(src_dir) {
        return load_from_gowork(&ws_root);
    }
    // Also check module root (src_dir may be a subdir whose parents lack go.work
    // until the module root — find_workspace_root already covers that).
    if module_root.join("go.work").is_file() {
        return load_from_gowork(module_root);
    }

    let mod_file = parse_mod_file(&module_root.join("go.mod")).map_err(|e| {
        Bail::new(BailReason::Io, format!("parse go.mod: {e}"))
    })?;
    Ok(Workspace {
        root: module_root.to_path_buf(),
        go_version: mod_file.go_version.clone(),
        modules: vec![WorkspaceModule {
            dir: module_root.to_path_buf(),
            mod_file,
        }],
        replaces: Vec::new(),
    })
}

fn load_from_gowork(ws_root: &Path) -> Result<Workspace, Bail> {
    let text = fs::read_to_string(ws_root.join("go.work")).map_err(|e| {
        Bail::new(BailReason::Io, format!("read go.work: {e}"))
    })?;
    let parsed = parse_gowork(&text)?;
    if parsed.uses.is_empty() {
        return Err(Bail::new(
            BailReason::GoWork,
            "go.work has no use directives",
        ));
    }

    let mut modules = Vec::new();
    for use_path in &parsed.uses {
        let dir = if Path::new(use_path).is_absolute() {
            PathBuf::from(use_path)
        } else {
            ws_root.join(use_path)
        };
        let dir = dir.canonicalize().map_err(|e| {
            Bail::new(
                BailReason::GoWork,
                format!("go.work use {use_path}: {e}"),
            )
        })?;
        let mod_file = parse_mod_file(&dir.join("go.mod")).map_err(|e| {
            Bail::new(
                BailReason::GoWork,
                format!("go.work use {}: parse go.mod: {e}", dir.display()),
            )
        })?;
        modules.push(WorkspaceModule { dir, mod_file });
    }

    Ok(Workspace {
        root: ws_root.to_path_buf(),
        go_version: parsed.go_version,
        modules,
        replaces: parsed.replaces,
    })
}

#[derive(Debug, Default)]
struct ParsedGoWork {
    go_version: Option<String>,
    uses: Vec<String>,
    replaces: Vec<Replace>,
}

fn parse_gowork(data: &str) -> Result<ParsedGoWork, Bail> {
    let mut out = ParsedGoWork::default();
    let mut block: Option<BlockKind> = None;

    for raw in data.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line == ")" {
            block = None;
            continue;
        }
        if line.starts_with("go ") {
            out.go_version = Some(line["go ".len()..].trim().to_string());
            continue;
        }
        if starts_directive(line, "use") {
            if let Some(rest) = open_block(line, "use") {
                block = Some(BlockKind::Use);
                if !rest.is_empty() {
                    out.uses.push(rest.to_string());
                }
            } else if let Some(path) = line.strip_prefix("use ") {
                out.uses.push(path.trim().to_string());
            }
            continue;
        }
        if starts_directive(line, "replace") {
            if let Some(rest) = open_block(line, "replace") {
                block = Some(BlockKind::Replace);
                if !rest.is_empty() {
                    if let Some(r) = parse_replace_line(rest) {
                        out.replaces.push(r);
                    }
                }
            } else if let Some(rest) = line.strip_prefix("replace ") {
                if let Some(r) = parse_replace_line(rest) {
                    out.replaces.push(r);
                }
            }
            continue;
        }
        match block {
            Some(BlockKind::Use) => out.uses.push(line.to_string()),
            Some(BlockKind::Replace) => {
                if let Some(r) = parse_replace_line(line) {
                    out.replaces.push(r);
                }
            }
            None => {}
        }
    }
    Ok(out)
}

#[derive(Clone, Copy)]
enum BlockKind {
    Use,
    Replace,
}

fn starts_directive(line: &str, name: &str) -> bool {
    line == name
        || line.starts_with(&format!("{name} "))
        || line.starts_with(&format!("{name}("))
}

fn open_block<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(name)?.trim_start();
    let rest = rest.strip_prefix('(')?.trim();
    if rest == ")" {
        return Some("");
    }
    Some(rest)
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

impl Workspace {
    /// Module that contains `dir` (longest path match), if any.
    pub fn module_containing(&self, dir: &Path) -> Option<&WorkspaceModule> {
        let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        self.modules
            .iter()
            .filter(|m| dir.starts_with(&m.dir))
            .max_by_key(|m| m.dir.as_os_str().len())
    }

    /// Longest-prefix workspace module whose module path matches `import_path`.
    pub fn module_for_import(&self, import_path: &str) -> Option<&WorkspaceModule> {
        self.modules
            .iter()
            .filter(|m| path_prefix_match(import_path, &m.mod_file.module_path))
            .max_by_key(|m| m.mod_file.module_path.len())
    }
}

fn path_prefix_match(import_path: &str, module_path: &str) -> bool {
    import_path == module_path
        || (import_path.starts_with(module_path)
            && import_path.as_bytes().get(module_path.len()) == Some(&b'/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prometheus_style_gowork() {
        let text = r#"
go 1.25.8

use (
	.
	./compliance
	./internal/tools
)

replace example.com/foo => ../foo
"#;
        let p = parse_gowork(text).unwrap();
        assert_eq!(p.go_version.as_deref(), Some("1.25.8"));
        assert_eq!(p.uses, vec![".", "./compliance", "./internal/tools"]);
        assert_eq!(p.replaces.len(), 1);
        assert_eq!(p.replaces[0].old_path, "example.com/foo");
    }
}
