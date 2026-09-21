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

/// Read the file back and scan it line by line, returning whether it could be.
///
/// SA9009 is a purely lexical rule — upstream only asks for the comment's text
/// and its column — so the source is a complete answer and the AST is not: the
/// production parse runs without `PARSE_COMMENTS` and keeps only *some* groups
/// (the leading file comment survives, everything else is dropped). Gating this
/// scan on "the file has no comments at all" therefore switched it off for
/// every file with a license header, which is every file in beats —
/// `filebeat/input/net/manager.go:40` writes `// go:generate moq …` under one.
///
/// The path comes from the `FileSet` rather than from indexing
/// `compiled_go_files` in step with `pass.files()`: the two lists do not have
/// to be in the same order, and a cgo package's `FileSet` name is the generated
/// file, which is what upstream reads too.
fn check_source_file(pass: &Pass<'_>, file: &File, pending: &mut Vec<(u32, String)>) -> bool {
    let start = file.file_start;
    let name = pass.fset().position_for(start, false).filename;
    if name.is_empty() {
        return false;
    }
    // Line the AST file up with `compiled_go_files` by *name*: the two lists do
    // not have to be in the same order, and the type checker already holds the
    // bytes, so nothing is read twice. The `FileSet` name can be relative (the
    // unit-test harness makes it so), which is why the path comes from the
    // package rather than from the name.
    let base = std::path::Path::new(&name).file_name();
    let idx = pass
        .pkg()
        .compiled_go_files
        .iter()
        .position(|p| p.file_name() == base);
    let owned;
    let src: &str = match idx.and_then(|i| pass.pkg().source_bytes(i)) {
        Some(bytes) => match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return false,
        },
        None => {
            let path = idx
                .and_then(|i| pass.pkg().compiled_go_files.get(i))
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from(&name));
            match std::fs::read_to_string(&path) {
                Ok(s) => {
                    owned = s;
                    &owned
                }
                Err(_) => return false,
            }
        }
    };
    let mut offset = start.0;
    if offset == 0 {
        offset = 1;
    }
    // `split_inclusive` keeps the newline, so the byte offsets stay exact
    // whatever the line endings are.
    for line in src.split_inclusive('\n') {
        let text = line.trim_end_matches(['\n', '\r']);
        if text.starts_with("//") {
            check_comment_text(pass, offset as u32, text, pending);
        }
        offset += line.len() as i64;
    }
    true
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut pending = Vec::new();
    for file in pass.files() {
        // The source is the complete answer; the AST is only the fallback for
        // a file that is not on disk.
        if check_source_file(pass, file, &mut pending) {
            continue;
        }
        for cg in comment_groups(file) {
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
