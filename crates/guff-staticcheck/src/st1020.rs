//! ST1020 — documentation of an exported function should start with the function's name.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1020`. Non-default in upstream.
//! Re-parses with `PARSE_COMMENTS` because load uses `Mode::NONE` (declaration
//! docs after the package clause are dropped otherwise).

use std::fs;
use std::sync::{Arc, OnceLock};

use guff::ast::{CommentGroup, Decl, Expr, File};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;
use guff_analysis::code::is_in_test_at;
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

fn receiver_type_name(ty: &Expr) -> Option<&str> {
    let mut t = ty;
    if let Expr::StarExpr(star) = t {
        t = &star.x;
    }
    match t {
        Expr::Ident(id) => Some(id.name.as_str()),
        Expr::IndexExpr(idx) => match &*idx.x {
            Expr::Ident(id) => Some(id.name.as_str()),
            _ => None,
        },
        Expr::IndexListExpr(idx) => match &*idx.x {
            Expr::Ident(id) => Some(id.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn line_pos(pass: &Pass<'_>, file: &File, line: i64) -> Option<u32> {
    let ft = pass.fset().file(file.pos())?;
    if line <= 0 || line as usize > ft.line_count() {
        return None;
    }
    Some(ft.line_start(line as usize).0 as u32)
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
            let Decl::FuncDecl(fd) = decl else {
                continue;
            };
            let Some(doc) = &fd.doc else {
                continue;
            };
            let Some(text) = doc_text(doc) else {
                continue;
            };
            if !is_exported(&fd.name.name) {
                continue;
            }
            if text.starts_with("Deprecated: ") {
                continue;
            }

            let mut kind = "function";
            if let Some(recv) = &fd.recv {
                kind = "method";
                let Some(field) = recv.list.first() else {
                    continue;
                };
                let Some(ty) = &field.ty else {
                    continue;
                };
                let Some(ident_name) = receiver_type_name(ty) else {
                    continue;
                };
                if !is_exported(ident_name) {
                    continue;
                }
            }

            let prefix = format!("{} ", fd.name.name);
            if text.starts_with(&prefix) {
                continue;
            }

            let line = re_fset.position(doc.pos()).line;
            let Some(mapped) = line_pos(pass, orig, line) else {
                continue;
            };
            pending.push((
                mapped,
                format!(
                    "comment on exported {kind} {} should be of the form \"{prefix}...\"",
                    fd.name.name
                ),
            ));
        }
    }

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn st1020_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1020",
        doc: "the documentation of an exported function should start with the function's name",
        url: "https://staticcheck.dev/docs/checks/#ST1020",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1020_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1020_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
