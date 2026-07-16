//! ST1000 — incorrect or missing package comment.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1000`.
//! Non-default in upstream; still useful for stylecheck coverage.

use std::sync::OnceLock;

use guff::ast::CommentGroup;
use guff_analysis::code::{is_in_test_at, is_main};
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn is_non_alpha(b: u8) -> bool {
    !b.is_ascii_alphanumeric()
}

fn doc_text(doc: &CommentGroup) -> Option<String> {
    let text = doc.text().trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if is_main(pass) {
        return Ok(None);
    }

    let mut pending: Vec<(u32, String)> = Vec::new();
    let mut has_docs = false;

    for file in pass.files() {
        if is_in_test_at(pass, file.package.0 as u32) {
            continue;
        }
        let Some(doc) = &file.doc else {
            continue;
        };
        let Some(text) = doc_text(doc) else {
            continue;
        };
        has_docs = true;
        let prefix = format!("Package {}", file.name.name);
        let ok = text.starts_with(&prefix)
            && (text.len() == prefix.len() || is_non_alpha(text.as_bytes()[prefix.len()]));
        if !ok {
            pending.push((
                doc.pos().0 as u32,
                format!(r#"package comment should be of the form "{prefix}...""#),
            ));
        }
    }

    if !has_docs {
        for file in pass.files() {
            if is_in_test_at(pass, file.package.0 as u32) {
                continue;
            }
            pending.push((
                file.package.0 as u32,
                "at least one file in a package should have a package comment".into(),
            ));
        }
    }

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn st1000_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1000",
        doc: "incorrect or missing package comment",
        url: "https://staticcheck.dev/docs/checks/#ST1000",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1000_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1000_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
