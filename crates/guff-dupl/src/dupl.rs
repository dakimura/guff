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
        // Upstream: `fsutils.ShortestRelPath(i.To.Filename(), "")` — the path
        // relative to the working directory, not the basename. The difference
        // is invisible until a config excludes by `text`: gitea drops dupl
        // findings matching `(?i)webhook`, which its `services/webhook/*.go`
        // duplicates carry only because the *path* is in the message. Eight
        // findings golangci-lint does not report.
        let to_name = shortest_rel_path(&issue.to.filename);
        let msg = format!(
            "{}-{} lines are duplicate of `{}:{}-{}`",
            issue.from.line_start,
            issue.from.line_end,
            to_name,
            issue.to.line_start,
            issue.to.line_end
        );
        pass.report(Diagnostic {
            pos,
            // Upstream builds `token.Position{Filename, Line}` and never sets
            // Column, so golangci prints column 0. Deriving it from the offset
            // gives 1, which is a difference the finding-set diff does not key
            // on and the check-level golden does.
            column: Some(0),
            message: msg,
            ..Diagnostic::default()
        });
    }

    Ok(None)
}

/// Port of golangci-lint's `fsutils.ShortestRelPath(path, "")`: the path
/// relative to the process working directory, with symlinks resolved. Falls
/// back to the path as given when either step fails, as there is nothing better
/// to say.
fn shortest_rel_path(path: &str) -> String {
    let p = Path::new(path);
    let resolved = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let Ok(wd) = std::env::current_dir() else {
        return path.to_string();
    };
    let wd = std::fs::canonicalize(&wd).unwrap_or(wd);
    match resolved.strip_prefix(&wd) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => resolved.to_string_lossy().into_owned(),
    }
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
