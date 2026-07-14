//! Text (line-number) output format — golangci-style
//! `file:line:col: message (analyzer)`.

use std::io::{self, Write};

use guff::position::FileSet;
use guff_analysis::Diagnostic;

use crate::exclude::Issue;

use super::Formatter;

/// golangci-style text line: `file:line:col: message (analyzer)`.
pub fn format_diagnostic_text(fset: &FileSet, analyzer: &str, diag: &Diagnostic) -> String {
    let loc = if diag.pos != 0 {
        let pos = fset.position(guff::Pos(diag.pos as i64));
        if pos.filename.is_empty() {
            "?:0:0".to_string()
        } else {
            format!("{}:{}:{}", pos.filename, pos.line, pos.column)
        }
    } else {
        "?:0:0".to_string()
    };
    format_with_loc(&loc, analyzer, &diag.message)
}

/// Format using explicit file:line:col (for post-processor issues without a token pos).
pub fn format_issue_text(filename: &str, line: i64, column: i64, analyzer: &str, message: &str) -> String {
    let loc = if filename.is_empty() {
        "?:0:0".to_string()
    } else {
        format!("{filename}:{line}:{column}")
    };
    format_with_loc(&loc, analyzer, message)
}

fn format_with_loc(loc: &str, analyzer: &str, message: &str) -> String {
    if analyzer.is_empty() {
        format!("{loc}: {message}")
    } else {
        format!("{loc}: {message} ({analyzer})")
    }
}

/// Plain text / line-number formatter (no colors).
///
/// Colors (`colored-line-number`) and source-line underlining are DEFERRED to R8.
#[derive(Debug, Default, Clone)]
pub struct TextFormatter {
    /// Whether to append ` (analyzer)` (golangci `print-linter-name`, default true).
    pub print_linter_name: bool,
}

impl TextFormatter {
    pub fn new() -> Self {
        Self {
            print_linter_name: true,
        }
    }
}

impl Formatter for TextFormatter {
    fn name(&self) -> &'static str {
        "text"
    }

    fn print(&self, issues: &[Issue], w: &mut dyn Write) -> io::Result<()> {
        for issue in issues {
            let analyzer = if self.print_linter_name {
                issue.analyzer.as_str()
            } else {
                ""
            };
            let line = format_issue_text(
                &issue.filename,
                issue.line,
                issue.column,
                analyzer,
                &issue.text,
            );
            writeln!(w, "{line}")?;
        }
        Ok(())
    }
}
