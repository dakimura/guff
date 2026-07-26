//! Diff-based issue filtering (golangci `Diff` processor / revgrep).
//!
//! Supports `issues.new`, `new-from-rev`, `new-from-merge-base`, `new-from-patch`,
//! and `whole-files`. Git is invoked via subprocess (no git2 dependency).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::exclude::Issue;

/// Config knobs that enable the Diff processor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffFilterSpec {
    pub new: bool,
    pub new_from_rev: Option<String>,
    pub new_from_merge_base: Option<String>,
    pub new_from_patch: Option<String>,
    pub whole_files: bool,
}

impl DiffFilterSpec {
    /// True when any `new*` option is set.
    pub fn enabled(&self) -> bool {
        self.new
            || self.new_from_rev.as_ref().is_some_and(|s| !s.is_empty())
            || self
                .new_from_merge_base
                .as_ref()
                .is_some_and(|s| !s.is_empty())
            || self.new_from_patch.as_ref().is_some_and(|s| !s.is_empty())
    }
}

/// Parsed new-side line ranges for one file (1-based inclusive).
#[derive(Debug, Clone, Default)]
pub struct DiffState {
    pub(crate) repo_root: PathBuf,
    pub(crate) whole_files: bool,
    /// Normalized repo-relative path → new-side line ranges.
    pub(crate) lines: HashMap<String, Vec<(i64, i64)>>,
    pub(crate) changed_files: HashSet<String>,
}

impl DiffState {
    /// Whether `issue` should be kept under this diff.
    pub fn keeps(&self, issue: &Issue) -> bool {
        let Some(rel) = self.rel_path(&issue.filename) else {
            return false;
        };
        if self.whole_files {
            return self.changed_files.contains(&rel);
        }
        let Some(ranges) = self.lines.get(&rel) else {
            return false;
        };
        let line = issue.line;
        if line <= 0 {
            // No line info: keep only under whole-files (handled above).
            return false;
        }
        ranges.iter().any(|&(start, end)| line >= start && line <= end)
    }

    fn rel_path(&self, filename: &str) -> Option<String> {
        let mut norm = filename.replace('\\', "/");
        while let Some(stripped) = norm.strip_prefix("./") {
            norm = stripped.to_string();
        }
        let path = Path::new(&norm);
        let root = self.repo_root.as_path();
        if let Ok(rel) = path.strip_prefix(root) {
            let s = rel.to_string_lossy().replace('\\', "/");
            if !s.is_empty() {
                return Some(s);
            }
        }
        if path.is_relative() {
            return Some(norm.trim_start_matches("./").to_string());
        }
        // Try canonicalize both sides.
        if let (Ok(abs), Ok(root_abs)) = (path.canonicalize(), root.canonicalize()) {
            if let Ok(rel) = abs.strip_prefix(&root_abs) {
                let s = rel.to_string_lossy().replace('\\', "/");
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        None
    }
}

/// Build a [`DiffState`] from `spec`, or `None` when diff filtering is disabled.
///
/// On git/patch failure returns `Err` so the caller can warn and skip filtering
/// (do not silently drop all issues).
pub fn build_diff_state(spec: &DiffFilterSpec) -> Result<Option<DiffState>, String> {
    if !spec.enabled() {
        return Ok(None);
    }

    let cwd = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
    let repo_root = git_toplevel(&cwd)?;

    let patch = if let Some(path) = spec.new_from_patch.as_ref().filter(|s| !s.is_empty()) {
        std::fs::read_to_string(path).map_err(|e| format!("read new-from-patch {path}: {e}"))?
    } else if let Some(ref_name) = spec
        .new_from_merge_base
        .as_ref()
        .filter(|s| !s.is_empty())
    {
        let base = git_merge_base(&repo_root, ref_name)?;
        git_diff_from(&repo_root, &base)?
    } else if let Some(rev) = spec.new_from_rev.as_ref().filter(|s| !s.is_empty()) {
        git_diff_from(&repo_root, rev)?
    } else if spec.new {
        // Working-tree changes (staged + unstaged), matching golangci `--new`.
        let mut staged = git_diff_args(&repo_root, &["--cached"])?;
        let unstaged = git_diff_args(&repo_root, &[])?;
        if !staged.is_empty() && !unstaged.is_empty() && !staged.ends_with('\n') {
            staged.push('\n');
        }
        staged.push_str(&unstaged);
        staged
    } else {
        return Ok(None);
    };

    let mut state = DiffState {
        repo_root,
        whole_files: spec.whole_files,
        lines: HashMap::new(),
        changed_files: HashSet::new(),
    };
    parse_unified_diff(&patch, &mut state);
    Ok(Some(state))
}

fn git_toplevel(cwd: &Path) -> Result<PathBuf, String> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("git rev-parse --show-toplevel: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git rev-parse --show-toplevel failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        return Err("empty git toplevel".into());
    }
    Ok(PathBuf::from(s))
}

fn git_merge_base(repo_root: &Path, reference: &str) -> Result<String, String> {
    let out = Command::new("git")
        .args(["merge-base", "HEAD", reference])
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("git merge-base: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git merge-base HEAD {reference} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        return Err(format!("empty merge-base for {reference}"));
    }
    Ok(s)
}

fn git_diff_from(repo_root: &Path, rev: &str) -> Result<String, String> {
    git_diff_args(repo_root, &[rev])
}

fn git_diff_args(repo_root: &Path, extra: &[&str]) -> Result<String, String> {
    let mut args = vec!["diff", "--no-color", "--unified=0"];
    args.extend_from_slice(extra);
    args.extend_from_slice(&["--", "*.go", "go.mod", "go.sum"]);
    let out = Command::new("git")
        .args(&args)
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("git diff: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse a unified diff into `state` (new-side paths and line ranges).
pub fn parse_unified_diff(patch: &str, state: &mut DiffState) {
    let mut current: Option<String> = None;
    for line in patch.lines() {
        if let Some(path) = parse_diff_git_b_path(line) {
            let norm = normalize_diff_path(&path);
            state.changed_files.insert(norm.clone());
            state.lines.entry(norm.clone()).or_default();
            current = Some(norm);
            continue;
        }
        if let Some(path) = parse_plus_plus_plus_path(line) {
            let norm = normalize_diff_path(&path);
            // Prefer +++ path when present (handles renames / /dev/null).
            if norm != "/dev/null" {
                state.changed_files.insert(norm.clone());
                state.lines.entry(norm.clone()).or_default();
                current = Some(norm);
            } else {
                current = None;
            }
            continue;
        }
        if let Some((start, count)) = parse_hunk_new_side(line) {
            let Some(ref path) = current else {
                continue;
            };
            if count == 0 {
                // Pure deletion hunk — no new-side lines.
                continue;
            }
            let end = start + count - 1;
            state
                .lines
                .entry(path.clone())
                .or_default()
                .push((start, end));
        }
    }
}

fn normalize_diff_path(path: &str) -> String {
    let p = path.replace('\\', "/");
    // Strip `b/` prefix from `diff --git` / `+++ b/...`.
    if let Some(rest) = p.strip_prefix("b/") {
        return rest.to_string();
    }
    if let Some(rest) = p.strip_prefix("a/") {
        return rest.to_string();
    }
    p
}

fn parse_diff_git_b_path(line: &str) -> Option<String> {
    // diff --git a/foo.go b/foo.go
    let rest = line.strip_prefix("diff --git ")?;
    let mut parts = rest.split_whitespace();
    let _a = parts.next()?;
    let b = parts.next()?;
    Some(b.to_string())
}

fn parse_plus_plus_plus_path(line: &str) -> Option<String> {
    // +++ b/path\t…  or +++ /dev/null
    let rest = line.strip_prefix("+++ ")?;
    let path = rest.split('\t').next()?.trim();
    if path.is_empty() {
        return None;
    }
    Some(path.to_string())
}

/// Parse `@@ -old,oldcount +new,newcounter @@` → (new_start, new_count).
fn parse_hunk_new_side(line: &str) -> Option<(i64, i64)> {
    let rest = line.strip_prefix("@@ ")?;
    let plus = rest.find('+')?;
    let after_plus = &rest[plus + 1..];
    let end = after_plus.find(' ').unwrap_or(after_plus.len());
    let spec = &after_plus[..end];
    // form: START or START,COUNT
    let mut parts = spec.split(',');
    let start: i64 = parts.next()?.parse().ok()?;
    let count: i64 = match parts.next() {
        Some(c) => c.parse().ok()?,
        None => 1,
    };
    Some((start, count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::Diagnostic;

    fn issue(file: &str, line: i64) -> Issue {
        Issue {
            from_linter: "revive".into(),
            analyzer: "revive".into(),
            text: "x".into(),
            severity: String::new(),
            filename: file.into(),
            line,
            column: 1,
            source_line: None,
            diagnostic: Diagnostic::default(),
        }
    }

    #[test]
    fn parse_hunk_ranges() {
        let patch = "\
diff --git a/pkg/a.go b/pkg/a.go
index 111..222 100644
--- a/pkg/a.go
+++ b/pkg/a.go
@@ -10,0 +11,2 @@
+line11
+line12
@@ -20 +22 @@
+line22
";
        let mut state = DiffState {
            repo_root: PathBuf::from("/repo"),
            whole_files: false,
            ..DiffState::default()
        };
        parse_unified_diff(patch, &mut state);
        assert!(state.changed_files.contains("pkg/a.go"));
        let ranges = state.lines.get("pkg/a.go").expect("ranges");
        assert_eq!(ranges, &[(11, 12), (22, 22)]);
    }

    #[test]
    fn whole_files_keeps_any_line_in_changed_file() {
        let mut state = DiffState {
            repo_root: PathBuf::from("/repo"),
            whole_files: true,
            ..DiffState::default()
        };
        parse_unified_diff(
            "diff --git a/x.go b/x.go\n--- a/x.go\n+++ b/x.go\n@@ -1 +1 @@\n+hi\n",
            &mut state,
        );
        assert!(state.keeps(&issue("/repo/x.go", 99)));
        assert!(!state.keeps(&issue("/repo/other.go", 1)));
    }

    #[test]
    fn line_mode_requires_hunk_overlap() {
        let mut state = DiffState {
            repo_root: PathBuf::from("/repo"),
            whole_files: false,
            ..DiffState::default()
        };
        parse_unified_diff(
            "diff --git a/x.go b/x.go\n--- a/x.go\n+++ b/x.go\n@@ -5,0 +6,1 @@\n+added\n",
            &mut state,
        );
        assert!(state.keeps(&issue("/repo/x.go", 6)));
        assert!(!state.keeps(&issue("/repo/x.go", 5)));
        assert!(!state.keeps(&issue("/repo/x.go", 7)));
    }

    #[test]
    fn relative_paths_match() {
        let mut state = DiffState {
            repo_root: PathBuf::from("/repo"),
            whole_files: true,
            ..DiffState::default()
        };
        parse_unified_diff(
            "diff --git a/sub/y.go b/sub/y.go\n--- a/sub/y.go\n+++ b/sub/y.go\n@@ -1 +1 @@\n+z\n",
            &mut state,
        );
        assert!(state.keeps(&issue("sub/y.go", 1)));
        assert!(state.keeps(&issue("./sub/y.go", 1)));
    }

    #[test]
    fn deletion_only_hunk_adds_file_but_no_lines() {
        let mut state = DiffState {
            repo_root: PathBuf::from("/repo"),
            whole_files: false,
            ..DiffState::default()
        };
        parse_unified_diff(
            "diff --git a/z.go b/z.go\n--- a/z.go\n+++ b/z.go\n@@ -3,1 +2,0 @@\n-gone\n",
            &mut state,
        );
        assert!(state.changed_files.contains("z.go"));
        assert!(!state.keeps(&issue("/repo/z.go", 2)));
        state.whole_files = true;
        assert!(state.keeps(&issue("/repo/z.go", 2)));
    }

    #[test]
    fn spec_enabled() {
        assert!(!DiffFilterSpec::default().enabled());
        assert!(DiffFilterSpec {
            new: true,
            ..Default::default()
        }
        .enabled());
        assert!(DiffFilterSpec {
            new_from_merge_base: Some("origin/main".into()),
            ..Default::default()
        }
        .enabled());
    }
}
