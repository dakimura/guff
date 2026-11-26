//! ST1022 — documentation of an exported variable or constant should start with its name.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1022`. Non-default in upstream.
//! Re-parses with `PARSE_COMMENTS` because load uses `Mode::NONE`.
//! Only package-level decls are checked (does not descend into functions).

use std::fs;
use std::sync::{Arc, OnceLock};

use guff::ast::{CommentGroup, Decl, File, Spec};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos};
use guff::token::Token;
use guff_analysis::code::{is_in_test_at, remap_reparsed_pos};
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

fn doc_text(doc: &CommentGroup) -> Option<String> {
    let text = doc.text().trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Map a position from the comment re-parse back into the analysis FileSet.
///
/// Mapping by line alone pins every finding to column 1 — the defect the
/// gocritic comment checkers had. `remap_reparsed_pos` carries the column.
fn doc_pos(pass: &Pass<'_>, file: &File, re_fset: &FileSet, pos: Pos) -> Option<u32> {
    remap_reparsed_pos(pass.fset(), file.pos(), re_fset, pos).map(|p| p.0 as u32)
}

fn reparse(path: &std::path::Path) -> Option<(Arc<FileSet>, File)> {
    let src = fs::read(path).ok()?;
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, &src, PARSE_COMMENTS).ok()?;
    Some((fset, file))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut pending: Vec<(u32, String)> = Vec::new();
    let paths = pass.pkg().compiled_go_files.clone();
    let n = pass.files().len();

    for i in 0..n {
        let orig = &pass.files()[i];
        let Some(path) = paths.get(i) else {
            continue;
        };
        if is_in_test_at(pass, orig.package.0 as u32) {
            continue;
        }
        let Some((re_fset, parsed)) = reparse(path) else {
            continue;
        };

        for decl in &parsed.decls {
            let Decl::GenDecl(gen) = decl else {
                continue;
            };
            if gen.tok != Some(Token::VAR) && gen.tok != Some(Token::CONST) {
                continue;
            }
            // Parenthesized or multi-name specs: don't guess intention.
            if gen.lparen.is_valid() {
                continue;
            }
            if gen.specs.len() != 1 {
                continue;
            }
            let Spec::ValueSpec(vs) = &gen.specs[0] else {
                continue;
            };
            if vs.names.len() != 1 {
                continue;
            }
            let name = vs.names[0].name.as_str();
            if !is_exported(name) {
                continue;
            }
            let Some(doc) = &gen.doc else {
                continue;
            };
            let Some(text) = doc_text(doc) else {
                continue;
            };
            let prefix = format!("{name} ");
            if text.starts_with(&prefix) {
                continue;
            }

            let kind = if gen.tok == Some(Token::CONST) {
                "const"
            } else {
                "var"
            };
            let Some(mapped) = doc_pos(pass, orig, &re_fset, doc.pos()) else {
                continue;
            };
            pending.push((
                mapped,
                format!(
                    "comment on exported {kind} {name} should be of the form \"{prefix}...\""
                ),
            ));
        }
    }

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn st1022_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1022",
        doc: "the documentation of an exported variable or constant should start with variable's name",
        url: "https://staticcheck.dev/docs/checks/#ST1022",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1022_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1022_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
