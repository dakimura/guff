//! Port of golangci-lint's `lll` (`pkg/golinters/lll`).
//!
//! Defaults match golangci-lint: `line-length=120`, `tab-width=1`.
//!
//! DEFERRED: `linters.settings.lll` wiring; `format:path` unrelated.

use std::fs;
use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

/// golangci-lint defaults for `linters.settings.lll`.
const LINE_LENGTH: usize = 120;
const TAB_WIDTH: usize = 1;

const GO_COMMENT_DIRECTIVE_PREFIX: &str = "//go:";

fn check_file(
    path: &std::path::Path,
    line_start: impl Fn(usize) -> Option<u32>,
    pending: &mut Vec<(u32, String)>,
) {
    let Ok(src) = fs::read_to_string(path) else {
        return;
    };

    let tab_spaces = " ".repeat(TAB_WIDTH);
    let mut multi_import = false;

    for (idx, raw_line) in src.lines().enumerate() {
        let line_number = idx + 1;
        let line = raw_line.replace('\t', &tab_spaces);

        if line.starts_with(GO_COMMENT_DIRECTIVE_PREFIX) {
            continue;
        }

        if line.starts_with("import") {
            multi_import = line.ends_with('(');
            continue;
        }

        if multi_import {
            if line == ")" {
                multi_import = false;
            }
            continue;
        }

        let line_len = line.chars().count();
        if line_len > LINE_LENGTH {
            let Some(pos) = line_start(line_number) else {
                continue;
            };
            pending.push((
                pos,
                format!(
                    "The line is {line_len} characters long, which exceeds the maximum of {LINE_LENGTH} characters."
                ),
            ));
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "lll requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    let fset = pass.fset().clone();
    let pkg = pass.pkg();

    for (i, file) in pass.files().iter().enumerate() {
        let Some(path) = pkg.compiled_go_files.get(i) else {
            continue;
        };
        if !path.is_file() {
            continue;
        }
        let Some(ft) = fset.file(file.pos()) else {
            continue;
        };
        check_file(
            path,
            |line| {
                if line == 0 || line > ft.line_count() {
                    return None;
                }
                Some(ft.line_start(line).0 as u32)
            },
            &mut pending,
        );
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "lll",
        doc: "reports long lines",
        url: "https://github.com/golangci/golangci-lint",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
