//! Apply analyzer [`SuggestedFix`] / [`TextEdit`] to source files on disk.
//!
//! Port of golangci-lint `pkg/result/processors/fixer.go` and go/analysis fix
//! application (offset-descending, overlap rejection).

use std::collections::{BTreeMap, HashSet};
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
    /// golangci groups edits by (file, linter), and excludes a whole linter
    /// from a file when its edits conflict — so the name has to travel with
    /// the edit.
    linter: String,
    start: usize,
    end: usize,
    new_text: String,
    /// Indices into the input `issues` slice. Normally one; deduplicating
    /// equivalent edits merges the issues that produced them, so that a
    /// package loaded twice does not leave the second copy reported as
    /// unfixed.
    issue_idxs: Vec<usize>,
}

/// `skipNoTextEdit`: an issue is unfixable only when *every* one of its fixes
/// is message-only. One fix with edits is enough to make it fixable.
fn skip_no_text_edit(fixes: &[guff_analysis::SuggestedFix]) -> bool {
    fixes.iter().all(|f| f.text_edits.is_empty())
}

/// `validateEdits`: sort by (start, end), drop *equivalent* duplicates, and
/// report whether any adjacent pair overlaps.
///
/// Equivalent edits are deduplicated rather than counted as a conflict — a
/// package loaded both as `P` and as `P [P.test]` yields every fix twice, and
/// upstream says so in a comment.
fn validate_edits(mut edits: Vec<ResolvedEdit>) -> (Vec<ResolvedEdit>, bool) {
    if edits.is_empty() {
        return (edits, false);
    }
    // `diff.SortEdits` is a *stable* sort by (start, end), which keeps several
    // insertions at one point in their original order.
    edits.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
    let equivalent = |a: &ResolvedEdit, b: &ResolvedEdit| {
        a.start == b.start && a.end == b.end && a.new_text == b.new_text
    };
    let mut unique = vec![edits[0].clone()];
    let mut invalid = false;
    for i in 1..edits.len() {
        let (prev, cur) = (&edits[i - 1], &edits[i]);
        if equivalent(prev, cur) {
            let merged = cur.issue_idxs.clone();
            unique
                .last_mut()
                .expect("unique is non-empty")
                .issue_idxs
                .extend(merged);
        } else {
            unique.push(cur.clone());
            if prev.end > cur.start {
                invalid = true;
            }
        }
    }
    (unique, invalid)
}

/// Apply suggested fixes from filtered `issues` to files on disk.
///
/// Port of golangci-lint `pkg/result/processors/fixer.go`. Three parts of that
/// algorithm are load-bearing, and all three were previously approximated by
/// go/analysis's simpler "sort descending, skip anything that overlaps":
///
/// * Edits are grouped by (file, linter). If any two edits **from one linter**
///   overlap in a file, that linter's *entire* edit set for the file is
///   dropped — not merely the overlapping edit. Each remaining pair of linters
///   is then checked the same way, and on conflict the alphabetically smaller
///   name is dropped.
/// * Every [`SuggestedFix`](guff_analysis::SuggestedFix) of an issue
///   contributes edits, not only the first.
/// * Equivalent edits are deduplicated instead of counted as a conflict.
///
/// Measured before this was ported: on one file where errorlint reports both
/// "comparing with ==" and "type assertion on error" for the same expression,
/// golangci-lint changed 0 lines and guff changed 4.
///
/// **One deliberate divergence.** Upstream returns only the issues that had no
/// text edits at all, so a finding whose linter lost the conflict pass is
/// neither fixed *nor reported* — it disappears from the output. guff keeps
/// reporting it. That is the same call
/// `compat/golden/cases/revive/ratchet.json` records for revive's
/// `importer.Default()` blindness: emulating an upstream defect that drops
/// true positives is not worth the parity.
///
/// `formatter` is applied to each rewritten file, matching upstream's
/// `p.formatter.Format(path, out)`. Pass `None` only where no formatting is
/// wanted at all.
///
/// Returns `(remaining_issues, fixes_applied)`.
pub fn apply_fixes(
    fset: &FileSet,
    issues: &[Issue],
    formatter: Option<&guff_fmt::MetaFormatter>,
) -> Result<(Vec<Issue>, usize), FixError> {
    // file -> linter -> edits. BTreeMap so the exclusion pass is deterministic;
    // upstream iterates a Go map here, whose order is randomized.
    let mut by_file: BTreeMap<String, BTreeMap<String, Vec<ResolvedEdit>>> = BTreeMap::new();

    for (issue_idx, issue) in issues.iter().enumerate() {
        let fixes = &issue.diagnostic.suggested_fixes;
        if fixes.is_empty() || skip_no_text_edit(fixes) {
            continue;
        }
        for fix in fixes {
            for edit in &fix.text_edits {
                if let Some(r) = resolve_edit(fset, edit, issue_idx, &issue.from_linter) {
                    by_file
                        .entry(r.filename.clone())
                        .or_default()
                        .entry(r.linter.clone())
                        .or_default()
                        .push(r);
                }
            }
        }
    }

    let mut applied_issues: HashSet<usize> = HashSet::new();

    for (filename, by_linter) in by_file {
        let linters: Vec<String> = by_linter.keys().cloned().collect();
        let mut excluded: HashSet<String> = HashSet::new();

        // Does any linter conflict with itself?
        for linter in &linters {
            if validate_edits(by_linter[linter].clone()).1 {
                excluded.insert(linter.clone());
            }
        }

        // Does any pair of different linters conflict? Upstream normalizes each
        // pair so that x < y and excludes x, the smaller name.
        for (j, y) in linters.iter().enumerate() {
            for x in &linters[..j] {
                if excluded.contains(x) || excluded.contains(y) {
                    continue;
                }
                let mut combined = by_linter[x].clone();
                combined.extend(by_linter[y].iter().cloned());
                if validate_edits(combined).1 {
                    excluded.insert(x.clone());
                }
            }
        }

        let mut kept: Vec<ResolvedEdit> = Vec::new();
        for linter in &linters {
            if !excluded.contains(linter) {
                kept.extend(by_linter[linter].iter().cloned());
            }
        }
        let (kept, _) = validate_edits(kept);

        // No early-out when `kept` is empty. Upstream keys `editsByPath` off
        // the file having had *any* fixable text edit, not off any surviving
        // one, so a file whose every edit lost the conflict pass is still read,
        // run through the formatter and written back. That write is not a
        // no-op: gofmt-ing a file nobody edited is a visible change, and it is
        // the whole diff upstream produces for a file where two checks of the
        // same linter want the same region.
        let path = Path::new(&filename);
        let mut content = fs::read_to_string(path).map_err(FixError::Io)?;
        // `kept` is sorted ascending and non-overlapping, so applying from the
        // end keeps every earlier offset valid.
        for edit in kept.iter().rev() {
            if edit.end > content.len() || edit.start > edit.end {
                continue;
            }
            content.replace_range(edit.start..edit.end, &edit.new_text);
            applied_issues.extend(edit.issue_idxs.iter().copied());
        }
        // Upstream runs the meta-formatter over every file it rewrote, which
        // with no formatter configured is plain gofmt. Without it the inserted
        // text keeps the indentation of the replacement string rather than the
        // file's.
        let out = match formatter {
            Some(f) => f
                .format(&filename, content.as_bytes())
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or(content),
            None => content,
        };
        fs::write(path, &out).map_err(FixError::Io)?;
    }

    let remaining: Vec<Issue> = issues
        .iter()
        .enumerate()
        .filter_map(|(i, issue)| {
            if applied_issues.contains(&i) {
                None
            } else {
                Some(issue.clone())
            }
        })
        .collect();

    Ok((remaining, applied_issues.len()))
}

fn resolve_edit(
    fset: &FileSet,
    edit: &TextEdit,
    issue_idx: usize,
    linter: &str,
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
        issue_idxs: vec![issue_idx],
        linter: linter.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::{Diagnostic, SuggestedFix};
    use std::io::Write;
    use tempfile::TempDir;

    /// Like [`issue_with_edits`] but names the reporting linter, which is what
    /// golangci's conflict pass groups by.
    fn issue_from(filename: &str, linter: &str, fixes: Vec<Vec<TextEdit>>) -> Issue {
        Issue {
            from_linter: linter.into(),
            analyzer: linter.into(),
            text: "test".into(),
            severity: String::new(),
            filename: filename.into(),
            line: 1,
            column: 1,
            source_line: None,
            diagnostic: Diagnostic {
                suggested_fixes: fixes
                    .into_iter()
                    .map(|text_edits| SuggestedFix {
                        message: "fix".into(),
                        text_edits,
                    })
                    .collect(),
                ..Diagnostic::default()
            },
        }
    }

    /// Writes `src` to a temp file and returns (dir, path, fset, base).
    fn scratch(src: &str) -> (TempDir, std::path::PathBuf, std::sync::Arc<FileSet>, i64) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("f.go");
        {
            let mut f = fs::File::create(&path).unwrap();
            write!(f, "{src}").unwrap();
        }
        let mut fset = FileSet::new();
        let file = fset.add_file(path.to_str().unwrap(), -1, src.len() as i64);
        let base = file.base();
        (dir, path, fset, base)
    }

    fn edit(base: i64, start: usize, end: usize, text: &str) -> TextEdit {
        TextEdit {
            pos: (base + start as i64) as u32,
            end: (base + end as i64) as u32,
            new_text: text.into(),
        }
    }

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

        let (remaining, n) = apply_fixes(&fset, &issues, None).unwrap();
        assert_eq!(n, 1);
        assert!(remaining.is_empty());
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "time.Sleep(42 * time.Nanosecond)");
    }

    #[test]
    fn overlapping_edits_from_one_linter_are_all_dropped() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("f.go");
        fs::write(&path, "0123456789").unwrap();

        let mut fset = FileSet::new();
        let file = fset.add_file(path.to_str().unwrap(), -1, 10);
        let base = file.base();

        // Two issues with overlapping ranges, both from one linter.
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

        let (remaining, n) = apply_fixes(&fset, &issues, None).unwrap();
        // This used to assert `n == 1` and `"0123B789"` — go/analysis's rule,
        // where the higher start offset wins and the overlapping edit is
        // skipped. golangci-lint drops *every* edit the linter contributed to
        // the file instead, so the file is untouched and both issues stand.
        assert_eq!(n, 0);
        assert_eq!(remaining.len(), 2);
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "0123456789");
    }

    // ---- golangci-lint `pkg/result/processors/fixer.go` conflict semantics ----
    //
    // These drive `apply_fixes` directly rather than through a linter: the
    // shape that provokes a conflict in golangci (errorlint reporting both
    // "comparing with ==" and "type assertion on error" on one expression)
    // cannot be produced by guff today, because guff's type-assertion check
    // carries no suggested fix at all. That missing fix is a separate gap; the
    // algorithm still has to be right before it lands.

    #[test]
    fn one_linter_conflicting_with_itself_loses_all_its_edits_for_the_file() {
        // Upstream drops the linter's *entire* edit set for the file, not just
        // the overlapping edit. go/analysis's rule — which guff used to follow —
        // would keep the non-overlapping one and apply it.
        let (_d, path, fset, base) = scratch("aaaabbbbcccc");
        let p = path.to_str().unwrap();
        let issues = vec![
            issue_from(p, "errorlint", vec![vec![edit(base, 0, 4, "X")]]),
            issue_from(p, "errorlint", vec![vec![edit(base, 2, 6, "Y")]]),
            issue_from(p, "errorlint", vec![vec![edit(base, 8, 12, "Z")]]),
        ];
        let (remaining, n) = apply_fixes(&fset, &issues, None).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "aaaabbbbcccc");
        assert_eq!(n, 0, "no edit from a self-conflicting linter may be applied");
        assert_eq!(remaining.len(), 3, "guff keeps reporting what it did not fix");
    }

    /// A gofmt-only `MetaFormatter`, which is what golangci builds when the
    /// user has configured no formatter linters.
    fn meta_gofmt() -> guff_fmt::MetaFormatter {
        guff_fmt::MetaFormatter::new(
            &["gofmt".into()],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .unwrap()
    }

    #[test]
    fn a_file_whose_every_edit_is_dropped_is_still_formatted_and_written() {
        // Upstream keys `editsByPath` off the file having had a fixable text
        // edit, not off one surviving the conflict pass, so it reads, formats
        // and writes the file even with nothing left to apply. That is the
        // entire diff golangci-lint produces when two checks of one linter want
        // the same region: the code is untouched and the file is gofmt'd.
        let src = "package main\n\nimport (\"errors\"; \"fmt\")\n\nvar _ = errors.New(fmt.Sprintf(\"x\"))\n";
        let (_d, path, fset, base) = scratch(src);
        let p = path.to_str().unwrap();
        // Two staticcheck edits over the same expression, as S1028 and S1039
        // produce for `errors.New(fmt.Sprintf("x"))`.
        let outer = src.find("errors.New").unwrap();
        let inner = src.find("fmt.Sprintf").unwrap();
        let issues = vec![
            issue_from(p, "staticcheck", vec![vec![edit(base, outer, src.len() - 1, "fmt.Errorf(\"x\")")]]),
            issue_from(p, "staticcheck", vec![vec![edit(base, inner, src.len() - 2, "\"x\"")]]),
        ];
        let (_remaining, n) = apply_fixes(&fset, &issues, Some(&meta_gofmt())).unwrap();
        assert_eq!(n, 0, "conflicting edits from one linter all lose");
        let after = fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("errors.New(fmt.Sprintf(\"x\"))"),
            "the code itself must be untouched, got:\n{after}"
        );
        assert!(
            after.contains("\t\"errors\"\n\t\"fmt\"\n"),
            "the file must still have been gofmt'd, got:\n{after}"
        );
    }

    #[test]
    fn a_conflict_between_two_linters_drops_the_alphabetically_smaller_one() {
        // Upstream normalizes each pair so x < y and excludes x.
        let (_d, path, fset, base) = scratch("aaaabbbbcccc");
        let p = path.to_str().unwrap();
        let issues = vec![
            issue_from(p, "errorlint", vec![vec![edit(base, 0, 6, "X")]]),
            issue_from(p, "zerologlint", vec![vec![edit(base, 4, 8, "Y")]]),
        ];
        let (_remaining, n) = apply_fixes(&fset, &issues, None).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "aaaaYcccc");
        assert_eq!(n, 1);
    }

    #[test]
    fn equivalent_edits_are_deduplicated_rather_than_treated_as_a_conflict() {
        // A package loaded as both `P` and `P [P.test]` yields every fix twice.
        // Upstream says so in a comment on `validateEdits`; treating the pair as
        // overlapping would silence the linter on every such package.
        let (_d, path, fset, base) = scratch("aaaabbbb");
        let p = path.to_str().unwrap();
        let issues = vec![
            issue_from(p, "errorlint", vec![vec![edit(base, 0, 4, "X")]]),
            issue_from(p, "errorlint", vec![vec![edit(base, 0, 4, "X")]]),
        ];
        let (_remaining, n) = apply_fixes(&fset, &issues, None).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "Xbbbb");
        assert_eq!(n, 2, "both issues count as fixed by the one surviving edit");
    }

    #[test]
    fn every_suggested_fix_contributes_edits_not_only_the_first() {
        // Upstream iterates all of `issue.SuggestedFixes`; guff used to take
        // `.first()`. Two non-overlapping fixes on one issue must both land.
        let (_d, path, fset, base) = scratch("aaaabbbb");
        let p = path.to_str().unwrap();
        let issues = vec![issue_from(
            p,
            "errorlint",
            vec![vec![edit(base, 0, 4, "X")], vec![edit(base, 4, 8, "Y")]],
        )];
        let (_remaining, n) = apply_fixes(&fset, &issues, None).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "XY");
        assert_eq!(n, 1);
    }

    #[test]
    fn an_issue_whose_fixes_are_all_message_only_is_left_alone() {
        // `skipNoTextEdit`: unfixable only when *every* fix is message-only.
        let (_d, path, fset, _base) = scratch("aaaabbbb");
        let p = path.to_str().unwrap();
        let issues = vec![issue_from(p, "errorlint", vec![vec![], vec![]])];
        let (remaining, n) = apply_fixes(&fset, &issues, None).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "aaaabbbb");
        assert_eq!(n, 0);
        assert_eq!(remaining.len(), 1);
    }

    #[test]
    fn non_overlapping_edits_from_one_linter_all_apply() {
        let (_d, path, fset, base) = scratch("aaaabbbbcccc");
        let p = path.to_str().unwrap();
        let issues = vec![
            issue_from(p, "errorlint", vec![vec![edit(base, 0, 4, "X")]]),
            issue_from(p, "errorlint", vec![vec![edit(base, 8, 12, "Z")]]),
        ];
        let (remaining, n) = apply_fixes(&fset, &issues, None).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "XbbbbZ");
        assert_eq!(n, 2);
        assert!(remaining.is_empty());
    }

}
