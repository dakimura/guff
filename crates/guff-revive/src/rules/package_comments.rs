//! `package-comments` — package comments should exist and follow conventions.
//!
//! Load uses `Mode::NONE` (no `PARSE_COMMENTS`), so `File.doc` is empty on the
//! type-checked AST. Re-parse with comments (same pattern as `blank-imports`)
//! and match upstream revive: a package comment on *any* non-test file silences
//! the missing-comment warning for the whole package.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use guff::ast::{CommentGroup, File};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{first_comment_line, has_prefix_insensitive, is_test_package};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    if is_test_package(&pass.pkg().name) {
        return Vec::new();
    }

    let paths = &pass.pkg().compiled_go_files;
    let mut files: Vec<(usize, Arc<FileSet>, File)> = Vec::new();
    for (i, report) in pass.files().iter().enumerate() {
        if paths
            .get(i)
            .is_some_and(|p| p.to_string_lossy().ends_with("_test.go"))
        {
            continue;
        }
        let Some(path) = paths.get(i) else {
            continue;
        };
        if let Some((fset, file)) = reparse_with_comments(path, pass.pkg().source_bytes(i)) {
            files.push((i, fset, file));
        } else {
            // Fall back to the type-checked AST (docs likely missing).
            files.push((i, Arc::clone(pass.fset()), report.clone()));
        }
    }
    if files.is_empty() {
        return Vec::new();
    }

    let pkg_name = &pass.pkg().name;
    let prefix = format!("Package {pkg_name} ");
    let mut failures = Vec::new();

    // Detached / form checks are per-file (upstream walks each file).
    for (fi, comments_fset, file) in &files {
        check_file_shape(
            pass,
            *fi,
            file,
            comments_fset,
            pkg_name,
            &prefix,
            &mut failures,
        );
    }

    // Missing package comment: once per package, only if no file has a doc.
    if files.iter().any(|(_, _, f)| !is_empty_doc(f.doc.as_ref())) {
        return failures;
    }

    // Prefer doc.go, then $package.go, then lexicographically first file.
    let report_idx = pick_missing_comment_file(&files, pkg_name);
    let Some((fi, comments_fset, file)) = files.iter().find(|(i, _, _)| *i == report_idx) else {
        return failures;
    };
    let report = &pass.files()[*fi];
    failures.push(Failure {
        rule: "package-comments",
        pos: remap_pos(pass, report, comments_fset, file.name.name_pos.0 as u32),
        message: "should have a package comment".into(),
        confidence: None,
    });
    failures
}

fn pick_missing_comment_file(files: &[(usize, Arc<FileSet>, File)], pkg_name: &str) -> usize {
    let path_name = |i: usize, f: &File| -> String {
        // Prefer basename from the FileSet when available; else package name.
        let _ = f;
        format!("{i}")
    };
    let mut doc_go: Option<usize> = None;
    let mut package_go: Option<usize> = None;
    let mut first: Option<(String, usize)> = None;
    for (fi, fset, file) in files {
        let name = fset
            .file(file.pos())
            .map(|ft| {
                Path::new(ft.name())
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string()
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| path_name(*fi, file));
        if name == "doc.go" {
            doc_go = Some(*fi);
        }
        if name == format!("{pkg_name}.go") {
            package_go = Some(*fi);
        }
        match &first {
            None => first = Some((name, *fi)),
            Some((prev, _)) if name < *prev => first = Some((name, *fi)),
            _ => {}
        }
    }
    doc_go
        .or(package_go)
        .or_else(|| first.map(|(_, i)| i))
        .unwrap_or(0)
}

fn check_file_shape(
    pass: &Pass<'_>,
    fi: usize,
    file: &File,
    comments_fset: &FileSet,
    pkg_name: &str,
    prefix: &str,
    failures: &mut Vec<Failure>,
) {
    let report = &pass.files()[fi];

    if let Some(detached) = detached_package_comment(file, comments_fset, prefix) {
        failures.push(Failure {
            rule: "package-comments",
            pos: remap_pos(pass, report, comments_fset, detached),
            message: "package comment is detached; there should be no blank lines between it and the package statement".into(),
            confidence: None,
        });
        return;
    }

    if is_empty_doc(file.doc.as_ref()) {
        // Missing comment is handled package-wide in `apply`.
        return;
    }

    let text = file.doc.as_ref().map(|d| d.text()).unwrap_or_default();
    if pkg_name != "main"
        && !text.starts_with(prefix)
        && !is_directive_comment(&text)
        && !has_prefix_insensitive(&first_comment_line(file.doc.as_ref()), prefix)
    {
        let pos = file
            .doc
            .as_ref()
            .map(|d| d.pos().0 as u32)
            .unwrap_or(file.package.0 as u32);
        failures.push(Failure {
            rule: "package-comments",
            pos: remap_pos(pass, report, comments_fset, pos),
            message: format!(r#"package comment should be of the form "{prefix}...""#),
            confidence: None,
        });
    }
}

fn reparse_with_comments(path: &Path, cached: Option<&[u8]>) -> Option<(Arc<FileSet>, File)> {
    let owned;
    let src: &[u8] = if let Some(b) = cached {
        b
    } else {
        owned = fs::read(path).ok()?;
        &owned
    };
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, src, PARSE_COMMENTS).ok()?;
    Some((fset, file))
}

fn remap_pos(pass: &Pass<'_>, report: &File, comments_fset: &FileSet, pos: u32) -> u32 {
    // When comments_fset is the package FileSet, positions already match.
    if std::ptr::eq(comments_fset as *const FileSet, pass.fset().as_ref() as *const FileSet) {
        return pos;
    }
    let p = comments_fset.position(guff::Pos(pos as i64));
    let Some(ft) = pass.fset().file(report.pos()) else {
        return pos;
    };
    if p.line <= 0 || p.line as usize > ft.line_count() {
        return pos;
    }
    let start = ft.line_start(p.line as usize).0 as u32;
    let col = p.column.max(1) as u32;
    start.saturating_add(col.saturating_sub(1))
}

fn is_empty_doc(doc: Option<&CommentGroup>) -> bool {
    doc.is_none_or(|d| d.text().trim().is_empty())
}

fn is_directive_comment(s: &str) -> bool {
    s.lines().all(|line| {
        let line = line.trim();
        line.is_empty() || line.starts_with("//go:")
    })
}

fn detached_package_comment(file: &File, fset: &FileSet, prefix: &str) -> Option<u32> {
    let mut last_before_pkg: Option<&CommentGroup> = None;
    for cg in &file.comments {
        if cg.pos().0 > file.package.0 {
            break;
        }
        last_before_pkg = Some(cg);
    }
    let cg = last_before_pkg?;
    if !cg.text().starts_with(prefix) {
        return None;
    }
    // Upstream: endPos.Line+1 < pkgPos.Line
    let end_line = fset.position(cg.end()).line;
    let pkg_line = fset.position(file.package).line;
    if end_line + 1 < pkg_line {
        // Anchor on the first blank line after the comment (upstream heuristic).
        Some(cg.end().0 as u32)
    } else {
        None
    }
}
