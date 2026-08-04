//! `blank-imports` — blank imports need justification outside main/test.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use guff::ast::{Decl, File, ImportSpec, Spec};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;
use guff::token::Token;
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_blank;

const MESSAGE: &str =
    "a blank import should be only in a main or test package, or have a comment justifying it";

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    if pass.pkg().name == "main" || pass.pkg().name.ends_with("_test") {
        return Vec::new();
    }
    let mut failures = Vec::new();
    let paths = pass.pkg().compiled_go_files.clone();
    let n = pass.files().len();
    for i in 0..n {
        let file = &pass.files()[i];
        // Upstream revive also allows blank imports in `*_test.go` (internal
        // tests use `package foo`, not `package foo_test`).
        if paths
            .get(i)
            .is_some_and(|p| p.to_string_lossy().ends_with("_test.go"))
        {
            continue;
        }
        // Load uses Mode::NONE (no PARSE_COMMENTS), so ImportSpec.comment is
        // usually unset. Re-parse with comments to match upstream revive, but
        // always report positions from the package's shared FileSet (`file`) —
        // a private FileSet would assign unrelated offsets and diagnostics would
        // map onto the wrong source files after JSON formatting.
        if let Some(path) = paths.get(i) {
            if let Some((cfset, with_comments)) =
                reparse_with_comments(path, pass.pkg().source_bytes(i))
            {
                check_file(file, &with_comments, &cfset, pass.fset(), &mut failures);
                continue;
            }
        }
        check_file(file, file, pass.fset(), pass.fset(), &mut failures);
    }
    failures
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

fn import_specs(file: &File) -> Vec<&ImportSpec> {
    file.decls
        .iter()
        .filter_map(|d| match d {
            Decl::GenDecl(g) if g.tok == Some(Token::IMPORT) => Some(&g.specs),
            _ => None,
        })
        .flatten()
        .filter_map(|s| match s {
            Spec::ImportSpec(imp) => Some(imp),
            _ => None,
        })
        .collect()
}

fn check_file(
    report: &File,
    comments: &File,
    comments_fset: &FileSet,
    package_fset: &FileSet,
    failures: &mut Vec<Failure>,
) {
    let report_imports = import_specs(report);
    let comment_imports = import_specs(comments);
    if report_imports.len() != comment_imports.len()
        || report_imports
            .iter()
            .zip(comment_imports.iter())
            .any(|(a, b)| a.path.value != b.path.value)
    {
        // Shape mismatch between the type-checked AST and the comment reparse;
        // fall back to the shared FileSet tree only (may miss comments).
        check_file(report, report, package_fset, package_fset, failures);
        return;
    }

    for (i, (report_imp, comment_imp)) in report_imports.iter().zip(comment_imports.iter()).enumerate()
    {
        if !report_imp.name.as_ref().is_some_and(is_blank) {
            continue;
        }

        if i > 0 {
            let prev = comment_imports[i - 1];
            // Upstream compares file line numbers, not raw Pos byte offsets.
            let prev_line = comments_fset.position(prev.path.pos()).line;
            let line = comments_fset.position(comment_imp.path.pos()).line;
            let prev_blank = prev.name.as_ref().is_some_and(is_blank);
            let prev_not_embed = prev.path.value != "\"embed\"";
            if prev_blank && prev_not_embed && line == prev_line + 1 {
                continue;
            }
        }

        if comment_imp.path.value == "\"embed\"" && file_has_embed_comment(comments) {
            continue;
        }

        if comment_imp.doc.is_none() && comment_imp.comment.is_none() {
            failures.push(Failure::new(
                "blank-imports",
                report_imp.path.pos().0 as u32,
                MESSAGE,
            ));
        }
    }
}

fn file_has_embed_comment(file: &File) -> bool {
    file.comments.iter().any(|cg| {
        cg.list
            .iter()
            .any(|c| c.text.starts_with("//go:embed "))
    })
}
