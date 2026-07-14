//! Apply analyzer [`SuggestedFix`] / [`TextEdit`] to source files on disk.
//!
//! Port of golangci-lint `pkg/result/processors/fixer.go` and go/analysis fix
//! application (offset-descending, overlap rejection).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use guff::position::FileSet;
use guff::Pos;
use guff_analysis::TextEdit;

use crate::exclude::Issue;

#[derive(Debug)]
pub enum FixError {
    Io(io::Error),
    Message(String),
}

impl std::fmt::Display for FixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Message(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for FixError {}

impl From<io::Error> for FixError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Clone)]
struct ResolvedEdit {
    filename: String,
    start: usize,
    end: usize,
    new_text: String,
    /// Index into the input `issues` slice.
    issue_idx: usize,
    /// Index within the first suggested fix's `text_edits`.
    edit_idx: usize,
}

/// Apply suggested fixes from filtered `issues` to files on disk.
///
/// Only the first [`SuggestedFix`](guff_analysis::SuggestedFix) of each
/// diagnostic is considered. An issue is removed from the returned list when
/// every text edit in that fix was applied without overlap.
///
/// Returns `(remaining_issues, fixes_applied)`.
pub fn apply_fixes(fset: &FileSet, issues: &[Issue]) -> Result<(Vec<Issue>, usize), FixError> {
    let mut resolved: Vec<ResolvedEdit> = Vec::new();
    let mut candidates: HashSet<usize> = HashSet::new();

    for (issue_idx, issue) in issues.iter().enumerate() {
        let Some(fix) = issue.diagnostic.suggested_fixes.first() else {
            continue;
        };
        if fix.text_edits.is_empty() {
            continue;
        }
        let mut any = false;
        for (edit_idx, edit) in fix.text_edits.iter().enumerate() {
            if let Some(r) = resolve_edit(fset, edit, issue_idx, edit_idx) {
                any = true;
                resolved.push(r);
            }
        }
        if any {
            candidates.insert(issue_idx);
        }
    }

    if resolved.is_empty() {
        return Ok((issues.to_vec(), 0));
    }

    let mut by_file: HashMap<String, Vec<ResolvedEdit>> = HashMap::new();
    for edit in resolved {
        by_file
            .entry(edit.filename.clone())
            .or_default()
            .push(edit);
    }

    let mut applied: HashSet<(usize, usize)> = HashSet::new();

    for (filename, mut edits) in by_file {
        edits.sort_by(|a, b| b.start.cmp(&a.start).then(b.end.cmp(&a.end)));

        let path = Path::new(&filename);
        let mut content = fs::read_to_string(path).map_err(FixError::Io)?;

        let mut min_start = content.len();
        for edit in &edits {
            if edit.end > content.len() || edit.start > edit.end {
                continue;
            }
            if edit.end <= min_start {
                content.replace_range(edit.start..edit.end, &edit.new_text);
                min_start = edit.start;
                applied.insert((edit.issue_idx, edit.edit_idx));
            }
        }

        fs::write(path, &content).map_err(FixError::Io)?;
    }

    let mut fixed_issues = HashSet::new();
    for &issue_idx in &candidates {
        let edit_count = issues[issue_idx]
            .diagnostic
            .suggested_fixes
            .first()
            .map(|f| f.text_edits.len())
            .unwrap_or(0);
        let all_applied = (0..edit_count).all(|i| applied.contains(&(issue_idx, i)));
        if all_applied {
            fixed_issues.insert(issue_idx);
        }
    }

    let remaining: Vec<Issue> = issues
        .iter()
        .enumerate()
        .filter_map(|(i, issue)| {
            if fixed_issues.contains(&i) {
                None
            } else {
                Some(issue.clone())
            }
        })
        .collect();

    Ok((remaining, fixed_issues.len()))
}

fn resolve_edit(
    fset: &FileSet,
    edit: &TextEdit,
    issue_idx: usize,
    edit_idx: usize,
) -> Option<ResolvedEdit> {
    if edit.pos == 0 {
        return None;
    }
    let start_pos = Pos(edit.pos as i64);
    let end_pos = if edit.end != 0 {
        Pos(edit.end as i64)
    } else {
        start_pos
    };
    let filename = fset.position(start_pos).filename;
    if filename.is_empty() {
        return None;
    }
    let file = fset.file(start_pos)?;
    let start = file.offset(start_pos) as usize;
    let end = file.offset(end_pos) as usize;
    if start > end {
        return None;
    }
    Some(ResolvedEdit {
        filename,
        start,
        end,
        new_text: edit.new_text.clone(),
        issue_idx,
        edit_idx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::{Diagnostic, SuggestedFix};
    use std::io::Write;
    use tempfile::TempDir;

    fn issue_with_edits(filename: &str, edits: Vec<TextEdit>) -> Issue {
        Issue {
            from_linter: "staticcheck".into(),
            analyzer: "SA1004".into(),
            text: "test".into(),
            severity: String::new(),
            filename: filename.into(),
            line: 1,
            column: 1,
            source_line: None,
            diagnostic: Diagnostic {
                suggested_fixes: vec![SuggestedFix {
                    message: "fix".into(),
                    text_edits: edits,
                }],
                ..Diagnostic::default()
            },
        }
    }

    #[test]
    fn apply_fixes_replaces_text() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("f.go");
        {
            let mut f = fs::File::create(&path).unwrap();
            write!(f, "time.Sleep(1)").unwrap();
        }

        let mut fset = FileSet::new();
        let file = fset.add_file(path.to_str().unwrap(), -1, 13);
        let base = file.base();
        let issues = vec![issue_with_edits(
            path.to_str().unwrap(),
            vec![TextEdit {
                pos: (base + 11) as u32,
                end: (base + 12) as u32,
                new_text: "42 * time.Nanosecond".into(),
            }],
        )];

        let (remaining, n) = apply_fixes(&fset, &issues).unwrap();
        assert_eq!(n, 1);
        assert!(remaining.is_empty());
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "time.Sleep(42 * time.Nanosecond)");
    }

    #[test]
    fn overlapping_edits_keep_first_in_descending_order() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("f.go");
        fs::write(&path, "0123456789").unwrap();

        let mut fset = FileSet::new();
        let file = fset.add_file(path.to_str().unwrap(), -1, 10);
        let base = file.base();

        // Two issues with overlapping ranges; higher start wins.
        let issues = vec![
            issue_with_edits(
                path.to_str().unwrap(),
                vec![TextEdit {
                    pos: (base + 2) as u32,
                    end: (base + 5) as u32,
                    new_text: "A".into(),
                }],
            ),
            issue_with_edits(
                path.to_str().unwrap(),
                vec![TextEdit {
                    pos: (base + 4) as u32,
                    end: (base + 7) as u32,
                    new_text: "B".into(),
                }],
            ),
        ];

        let (remaining, n) = apply_fixes(&fset, &issues).unwrap();
        assert_eq!(n, 1);
        assert_eq!(remaining.len(), 1);
        let content = fs::read_to_string(&path).unwrap();
        // Higher start offset (4) wins; overlapping edit at 2 is skipped.
        assert_eq!(content, "0123B789");
    }
}
