//! `blank-imports` — blank imports need justification outside main/test.

use std::fs;
use std::path::Path;

use guff::ast::{Decl, File, Spec};
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
        // Load uses Mode::NONE (no PARSE_COMMENTS), so ImportSpec.comment is
        // usually unset. Re-parse with comments to match upstream revive.
        if let Some(path) = paths.get(i) {
            if let Some(with_comments) = reparse_with_comments(path) {
                check_file(&with_comments, &mut failures);
                continue;
            }
        }
        check_file(file, &mut failures);
    }
    failures
}

fn reparse_with_comments(path: &Path) -> Option<File> {
    let src = fs::read(path).ok()?;
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    parse_file(&fset, name, &src, PARSE_COMMENTS).ok()
}

fn check_file(file: &File, failures: &mut Vec<Failure>) {
    let imports: Vec<_> = file
        .decls
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
        .collect();

    for (i, imp) in imports.iter().enumerate() {
        if !imp.name.as_ref().is_some_and(is_blank) {
            continue;
        }

        if i > 0 {
            let prev = imports[i - 1];
            let prev_line = prev.path.pos().0;
            let line = imp.path.pos().0;
            let prev_blank = prev.name.as_ref().is_some_and(is_blank);
            let prev_not_embed = prev.path.value != "\"embed\"";
            if prev_blank && prev_not_embed && line == prev_line + 1 {
                continue;
            }
        }

        if imp.path.value == "\"embed\"" && file_has_embed_comment(file) {
            continue;
        }

        if imp.doc.is_none() && imp.comment.is_none() {
            failures.push(Failure::new(
                "blank-imports",
                imp.path.pos().0 as u32,
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
