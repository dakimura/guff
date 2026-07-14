//! Issue output formatters (golangci `pkg/printers` equivalent).
//!
//! R6: [`Formatter`] + text. R7: JSON. R8: colored / checkstyle / sarif / tab /
//! github-actions.

mod checkstyle;
mod color;
mod github;
mod json;
mod sarif;
mod severity;
mod tab;
mod text;

use std::io::{self, Write};

use crate::exclude::Issue;

pub use checkstyle::CheckstyleFormatter;
pub use github::GithubActionsFormatter;
pub use json::{JsonFormatter, JsonReport, JsonWarning};
pub use sarif::SarifFormatter;
pub use tab::TabFormatter;
pub use text::{format_diagnostic_text, format_issue_text, TextFormatter};

/// Prints a slice of issues to a writer.
pub trait Formatter {
    fn name(&self) -> &'static str;
    fn print(&self, issues: &[Issue], w: &mut dyn Write) -> io::Result<()>;
}

/// Supported `--out-format` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputFormatKind {
    /// Plain `file:line:col: message (analyzer)` (also `line-number`).
    Text,
    /// Colored text + source line/caret when available (`colored-line-number`).
    Colored,
    /// golangci-lint JSON schema (`{"Issues":[...],"Report":...}`).
    Json,
    /// Checkstyle XML (`version="5.0"`).
    Checkstyle,
    /// SARIF 2.1.0 JSON.
    Sarif,
    /// Tab-aligned columns.
    Tab,
    /// Tab + colors (`colored-tab`).
    ColoredTab,
    /// GitHub Actions workflow commands (`::error file=…`).
    GithubActions,
}

impl OutputFormatKind {
    /// Parse a single format name (guff + golangci aliases).
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "text" | "line-number" => Ok(Self::Text),
            "colored-line-number" | "colored" => Ok(Self::Colored),
            "json" => Ok(Self::Json),
            "checkstyle" => Ok(Self::Checkstyle),
            "sarif" => Ok(Self::Sarif),
            "tab" => Ok(Self::Tab),
            "colored-tab" => Ok(Self::ColoredTab),
            "github-actions" | "github" => Ok(Self::GithubActions),
            other => Err(format!(
                "unknown output format {other:?}; supported: text, line-number, \
                 colored-line-number, json, checkstyle, sarif, tab, colored-tab, \
                 github-actions"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Colored => "colored-line-number",
            Self::Json => "json",
            Self::Checkstyle => "checkstyle",
            Self::Sarif => "sarif",
            Self::Tab => "tab",
            Self::ColoredTab => "colored-tab",
            Self::GithubActions => "github-actions",
        }
    }

    pub fn formatter(self) -> Box<dyn Formatter> {
        match self {
            Self::Text => Box::new(TextFormatter::new()),
            Self::Colored => Box::new(TextFormatter::colored()),
            Self::Json => Box::new(JsonFormatter::new()),
            Self::Checkstyle => Box::new(CheckstyleFormatter::new()),
            Self::Sarif => Box::new(SarifFormatter::new()),
            Self::Tab => Box::new(TabFormatter::new()),
            Self::ColoredTab => Box::new(TabFormatter::colored()),
            Self::GithubActions => Box::new(GithubActionsFormatter::new()),
        }
    }
}

/// Resolve CLI `--out-format` values (repeatable). Empty → `[Text]`.
///
/// `format:path` suffixes are accepted for golangci compatibility; writing to a
/// path instead of the shared writer is DEFERRED (still print to `w`).
pub fn resolve_out_formats(cli: &[String]) -> Result<Vec<OutputFormatKind>, String> {
    if cli.is_empty() {
        return Ok(vec![OutputFormatKind::Text]);
    }
    let mut out = Vec::with_capacity(cli.len());
    for raw in cli {
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
            OutputFormatKind::Colored
        );
    }

    #[test]
    fn parse_r8_formats() {
        assert_eq!(OutputFormatKind::parse("json").unwrap(), OutputFormatKind::Json);
        assert_eq!(
            OutputFormatKind::parse("checkstyle").unwrap(),
            OutputFormatKind::Checkstyle
        );
        assert_eq!(OutputFormatKind::parse("sarif").unwrap(), OutputFormatKind::Sarif);
        assert_eq!(OutputFormatKind::parse("tab").unwrap(), OutputFormatKind::Tab);
        assert_eq!(
            OutputFormatKind::parse("colored-tab").unwrap(),
            OutputFormatKind::ColoredTab
        );
        assert_eq!(
            OutputFormatKind::parse("github-actions").unwrap(),
            OutputFormatKind::GithubActions
        );
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
        assert_eq!(
            resolve_out_formats(&["json:/tmp/out.json".into()]).unwrap(),
            vec![OutputFormatKind::Json]
        );
        assert_eq!(
            resolve_out_formats(&["checkstyle:/tmp/cs.xml".into()]).unwrap(),
            vec![OutputFormatKind::Checkstyle]
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

    #[test]
    fn json_formatter_via_print_issues() {
        let mut buf = Vec::new();
        print_issues(&[OutputFormatKind::Json], &[sample_issue()], &mut buf).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(v["Issues"][0]["FromLinter"], "errcheck");
        assert_eq!(v["Issues"][0]["Text"], "unchecked error");
        assert!(v["Report"].is_null());
    }

    #[test]
    fn github_actions_via_print_issues() {
        let mut buf = Vec::new();
        print_issues(
            &[OutputFormatKind::GithubActions],
            &[sample_issue()],
            &mut buf,
        )
        .unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(
            s,
            "::error file=bad.go,line=5,col=2::unchecked error (errcheck)\n"
        );
    }
}
