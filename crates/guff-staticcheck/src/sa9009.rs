//! SA9009 — ineffectual Go compiler directive.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa9009`.

use std::sync::OnceLock;

use guff::ast::{CommentGroup, File};
use guff::position::Pos;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

/// Upstream walks `File.Comments` and nothing else. `File.Doc` and each
/// declaration's `Doc` are *views into* that same list, so adding them reported
/// every directive in a doc comment a second time. `issues.uniq-by-line` (on by
/// default) merged the pair, which kept it out of every gate but the golden
/// tier, where uniq-by-line is off.
fn comment_groups(file: &File) -> Vec<&CommentGroup> {
    file.comments.iter().collect()
}

fn check_comment_text(pass: &Pass<'_>, slash: u32, text: &str, pending: &mut Vec<(u32, String)>) {
    if !text.starts_with("//") {
        return;
    }
    if pass.fset().position_for(Pos(slash as i64), false).column != 1 {
        return;
    }
    let trimmed = text[2..].trim_start_matches([' ', '\t']);
    if trimmed.len() == text.len() - 2 {
        return;
    }
    if !trimmed.starts_with("go:") {
        return;
    }
    let rest = &trimmed[3..];
    if rest.is_empty() {
        return;
    }
    let Some(first) = rest.chars().next() else {
        return;
    };
    if !first.is_ascii_lowercase() {
        return;
    }
    pending.push((
        slash,
        format!("ineffectual compiler directive due to extraneous space: {text:?}"),
    ));
}

fn check_source_file(pass: &Pass<'_>, file_idx: usize, pending: &mut Vec<(u32, String)>) {
    let Some(path) = pass.pkg().compiled_go_files.get(file_idx) else {
        return;
    };
    let Ok(src) = std::fs::read_to_string(path) else {
        return;
    };
    let Some(file) = pass.files().get(file_idx) else {
        return;
    };
    let mut offset = file.file_start.0;
    if offset == 0 {
        offset = 1;
    }
    for line in src.lines() {
        let text = line.trim_end();
        if text.starts_with("//") {
            check_comment_text(pass, offset as u32, text, pending);
        }
        offset += line.len() as i64 + 1;
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut pending = Vec::new();
    for (file_idx, file) in pass.files().iter().enumerate() {
        let groups = comment_groups(file);
        if groups.is_empty() {
            check_source_file(pass, file_idx, &mut pending);
            continue;
        }
        for cg in groups {
            for c in &cg.list {
                check_comment_text(pass, c.slash.0 as u32, &c.text, &mut pending);
            }
        }
    }
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa9009_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA9009",
        doc: "ineffectual Go compiler directive",
        url: "https://staticcheck.dev/docs/checks/#SA9009",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa9009_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa9009_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
