//! `misspell` analyzer — flags commonly misspelled English words in Go sources.

use std::fs;
use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff::position::Pos;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::replacer::{Diff, Replacer};

static REPLACER: OnceLock<Replacer> = OnceLock::new();

fn replacer() -> &'static Replacer {
    REPLACER.get_or_init(Replacer::new)
}

fn report_diff(pass: &mut Pass<'_>, file_pos: Pos, diff: &Diff) {
    let fset = pass.fset();
    let Some(ft) = fset.file(file_pos) else {
        return;
    };
    if diff.line < 1 || diff.line > ft.line_count() {
        return;
    }
    let start = ft.line_start(diff.line).0 as u32 + diff.column as u32;
    let end = start + diff.original.len() as u32;
    pass.report(Diagnostic {
        pos: start,
        end,
        message: format!(
            "`{}` is a misspelling of `{}`",
            diff.original, diff.corrected
        ),
        suggested_fixes: vec![SuggestedFix {
            message: String::new(),
            text_edits: vec![TextEdit {
                pos: start,
                end,
                new_text: diff.corrected.clone(),
            }],
        }],
        ..Diagnostic::default()
    });
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "misspell requires inspect analyzer".to_string())?;

    let replacer = replacer();
    let paths: Vec<_> = pass.pkg().compiled_go_files.clone();
    let mut pending: Vec<(Pos, Diff)> = Vec::new();

    for (i, file) in pass.files().iter().enumerate() {
        let Some(path) = paths.get(i) else {
            continue;
        };
        if !path.is_file() {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let file_pos = file.pos();
        for diff in replacer.find_diffs(&content) {
            pending.push((file_pos, diff));
        }
    }

    for (file_pos, diff) in pending {
        report_diff(pass, file_pos, &diff);
    }

    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "misspell",
        doc: "Finds commonly misspelled English words",
        url: "https://github.com/golangci/misspell",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
