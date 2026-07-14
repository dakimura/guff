//! Issue output formatters (golangci `pkg/printers` equivalent).
//!
//! R6 introduces the [`Formatter`] trait and the text formatter.
//! JSON is R7; colored / checkstyle / sarif / etc. are R8.

mod text;

use std::io::{self, Write};

use crate::exclude::Issue;

pub use text::{format_diagnostic_text, format_issue_text, TextFormatter};

/// Prints a slice of issues to a writer.
pub trait Formatter {
    fn name(&self) -> &'static str;
    fn print(&self, issues: &[Issue], w: &mut dyn Write) -> io::Result<()>;
}

/// Supported `--out-format` names for this release.
///
/// Additional formats land in R7 (json) / R8 (colored, checkstyle, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputFormatKind {
    /// Plain `file:line:col: message (analyzer)` (also `line-number`).
    Text,
}

impl OutputFormatKind {
    /// Parse a single format name. Accepts guff `text` and golangci aliases
    /// that currently render as text (`line-number`, `colored-line-number`).
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "text" | "line-number" => Ok(Self::Text),
            // Colors DEFERRED to R8; for now emit the same as text.
            "colored-line-number" => Ok(Self::Text),
            "json" => Err(
                "output format \"json\" is not implemented yet (planned R7); use --out-format text"
                    .into(),
            ),
            other => Err(format!(
                "unknown output format {other:?}; supported: text, line-number \
                 (json/checkstyle/sarif/… come in later milestones)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
        }
    }

    pub fn formatter(self) -> Box<dyn Formatter> {
        match self {
            Self::Text => Box::new(TextFormatter::new()),
        }
    }
}

/// Resolve CLI `--out-format` values (repeatable). Empty → `[Text]`.
pub fn resolve_out_formats(cli: &[String]) -> Result<Vec<OutputFormatKind>, String> {
    if cli.is_empty() {
        return Ok(vec![OutputFormatKind::Text]);
    }
    let mut out = Vec::with_capacity(cli.len());
    for raw in cli {
        // golangci also accepts `format:path`; path writing is DEFERRED (R8).
        let name = raw.split_once(':').map(|(n, _)| n).unwrap_or(raw.as_str());
        out.push(OutputFormatKind::parse(name)?);
    }
    Ok(out)
}

/// Print `issues` with each selected formatter (same writer for now).
pub fn print_issues(
    formats: &[OutputFormatKind],
    issues: &[Issue],
    w: &mut dyn Write,
) -> io::Result<usize> {
    if formats.is_empty() {
        TextFormatter::new().print(issues, w)?;
        return Ok(issues.len());
    }
    for kind in formats {
        kind.formatter().print(issues, w)?;
    }
    Ok(issues.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::Diagnostic;

    fn sample_issue() -> Issue {
        Issue {
            from_linter: "errcheck".into(),
            analyzer: "errcheck".into(),
            text: "unchecked error".into(),
            severity: String::new(),
            filename: "bad.go".into(),
            line: 5,
            column: 2,
            source_line: None,
            diagnostic: Diagnostic {
                message: "unchecked error".into(),
                ..Diagnostic::default()
            },
        }
    }

    #[test]
    fn parse_text_aliases() {
        assert_eq!(OutputFormatKind::parse("text").unwrap(), OutputFormatKind::Text);
        assert_eq!(
            OutputFormatKind::parse("line-number").unwrap(),
            OutputFormatKind::Text
        );
        assert_eq!(
            OutputFormatKind::parse("colored-line-number").unwrap(),
            OutputFormatKind::Text
        );
    }

    #[test]
    fn parse_json_deferred() {
        let err = OutputFormatKind::parse("json").unwrap_err();
        assert!(err.contains("R7") || err.contains("not implemented"));
    }

    #[test]
    fn resolve_default_is_text() {
        assert_eq!(resolve_out_formats(&[]).unwrap(), vec![OutputFormatKind::Text]);
    }

    #[test]
    fn resolve_strips_path_suffix() {
        assert_eq!(
            resolve_out_formats(&["text:/tmp/out.txt".into()]).unwrap(),
            vec![OutputFormatKind::Text]
        );
    }

    #[test]
    fn text_formatter_matches_legacy_line() {
        let mut buf = Vec::new();
        TextFormatter::new()
            .print(&[sample_issue()], &mut buf)
            .unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s, "bad.go:5:2: unchecked error (errcheck)\n");
    }
}
