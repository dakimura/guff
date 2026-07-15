//! `blank-imports` — blank imports need justification outside main/test.

use guff::ast::{Decl, File, Spec};
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
    for file in pass.files() {
        check_file(file, &mut failures);
    }
    failures
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
            failures.push(Failure {
                rule: "blank-imports",
                pos: imp.path.pos().0 as u32,
                message: MESSAGE.into(),
            });
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
