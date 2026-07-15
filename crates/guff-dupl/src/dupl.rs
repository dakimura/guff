//! `dupl` analyzer — detects duplicate code fragments.

use std::path::Path;
use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn};

use crate::engine::{self, DEFAULT_THRESHOLD};

/// Per-linter options (`linters.settings.dupl`).
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub threshold: i32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "dupl requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<Options>("dupl")
        .copied()
        .unwrap_or_default();

    let paths: Vec<&Path> = pass
        .pkg()
        .compiled_go_files
        .iter()
        .filter(|p| p.is_file())
        .map(|p| p.as_path())
        .collect();
    if paths.is_empty() {
        return Ok(None);
    }

    let issues = engine::run(&paths, options.threshold)
        .map_err(|e| format!("dupl: {e}"))?;

    for issue in issues {
        let pos = line_pos(pass, &issue.from.filename, issue.from.line_start);
        let msg = format!(
            "{}-{} lines are duplicate of {}:{}-{}",
            issue.from.line_start,
            issue.from.line_end,
            issue.to.filename,
            issue.to.line_start,
            issue.to.line_end
        );
        pass.report(Diagnostic {
            pos,
            message: msg,
            ..Diagnostic::default()
        });
    }

    Ok(None)
}

fn line_pos(pass: &Pass<'_>, filename: &str, line: i32) -> u32 {
    let fset = pass.fset();
    for (i, file) in pass.files().iter().enumerate() {
        let Some(path) = pass.pkg().compiled_go_files.get(i) else {
            continue;
        };
        if path.to_string_lossy() == filename
            || path.file_name().and_then(|s| s.to_str()) == Some(filename)
        {
            let file_pos = file.pos();
            let Some(ft) = fset.file(file_pos) else {
                break;
            };
            if line >= 1 && line <= ft.line_count() as i32 {
                return ft.line_start(line as usize).0 as u32;
            }
        }
    }
    pass.files()
        .first()
        .map(|f| f.pos().0 as u32)
        .unwrap_or(0)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "dupl",
        doc: "Detects duplicate fragments of code",
        url: "https://github.com/mibk/dupl",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
