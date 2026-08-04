//! `package-comments` — package comments should exist and follow conventions.

use guff::ast::CommentGroup;
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{first_comment_line, has_prefix_insensitive, is_test_package};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    if is_test_package(&pass.pkg().name) {
        return Vec::new();
    }
    let mut failures = Vec::new();
    let mut warned_missing = false;
    let paths = &pass.pkg().compiled_go_files;
    for (i, file) in pass.files().iter().enumerate() {
        // Upstream revive skips `*_test.go` entirely (`lint.File.IsTest`).
        if paths
            .get(i)
            .is_some_and(|p| p.to_string_lossy().ends_with("_test.go"))
        {
            continue;
        }
        check_file(file, &pass.pkg().name, &mut failures, &mut warned_missing);
    }
    failures
}

fn check_file(
    file: &guff::ast::File,
    pkg_name: &str,
    failures: &mut Vec<Failure>,
    warned_missing: &mut bool,
) {
    let prefix = format!("Package {pkg_name} ");

    if let Some(detached) = detached_package_comment(file, &prefix) {
        failures.push(Failure {
            rule: "package-comments",
            pos: detached as u32,
            message: "package comment is detached; there should be no blank lines between it and the package statement".into(),
            confidence: None,
        });
        return;
    }

    if is_empty_doc(file.doc.as_ref()) {
        if !*warned_missing {
            failures.push(Failure {
                rule: "package-comments",
                pos: file.name.name_pos.0 as u32,
                message: "should have a package comment".into(),
            confidence: None,
        });
            *warned_missing = true;
        }
        return;
    }

    let text = file.doc.as_ref().map(|d| d.text()).unwrap_or_default();
    if pkg_name != "main"
        && !text.starts_with(&prefix)
        && !is_directive_comment(&text)
        && !has_prefix_insensitive(&first_comment_line(file.doc.as_ref()), &prefix)
    {
        failures.push(Failure {
            rule: "package-comments",
            pos: file.doc.as_ref().map(|d| d.pos().0).unwrap_or(file.package.0) as u32,
            message: format!(r#"package comment should be of the form "{prefix}...""#),
            confidence: None,
        });
    }
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

fn detached_package_comment(file: &guff::ast::File, prefix: &str) -> Option<i64> {
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
    // Heuristic: detached if comment group ends well before package line.
    let gap = file.package.0 - cg.end().0;
    if gap > 4 {
        Some(cg.end().0)
    } else {
        None
    }
}
