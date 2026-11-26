//! GitHub Actions workflow-command output.
//!
//! Format (restored golangci `pkg/printers/githubaction.go`, removed in v2 but
//! still useful for CI annotations):
//! `::error file=app.js,line=10,col=15::Something went wrong (linter)`

use std::io::{self, Write};

use crate::exclude::Issue;

use super::Formatter;

const DEFAULT_SEVERITY: &str = "error";

/// GitHub Actions annotation formatter (`--out-format github-actions`).
#[derive(Debug, Default, Clone)]
pub struct GithubActionsFormatter;

impl GithubActionsFormatter {
    pub fn new() -> Self {
        Self
    }
}

impl Formatter for GithubActionsFormatter {
    fn name(&self) -> &'static str {
        "github-actions"
    }

    fn print(&self, issues: &[Issue], w: &mut dyn Write) -> io::Result<()> {
        for issue in issues {
            writeln!(w, "{}", format_issue_as_github(issue))?;
        }
        Ok(())
    }
}

fn format_issue_as_github(issue: &Issue) -> String {
    let severity = if issue.severity.is_empty() {
        DEFAULT_SEVERITY
    } else {
        issue.severity.as_str()
    };

    // Convert backslashes → forward slashes so Windows paths annotate correctly
    // (golangci `filepath.ToSlash`).
    let file = issue.filename.replace('\\', "/");

    let mut out = format!("::{severity} file={file},line={}", issue.line);
    if issue.column != 0 {
        out.push_str(&format!(",col={}", issue.column));
    }
    out.push_str(&format!("::{} ({})", issue.text, issue.from_linter));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::Diagnostic;

    fn sample(from: &str, text: &str, file: &str, line: i64, col: i64, severity: &str) -> Issue {
        Issue {
            from_linter: from.into(),
            analyzer: from.into(),
            text: text.into(),
            severity: severity.into(),
            filename: file.into(),
            line,
            column: col,
            source_line: None,
            diagnostic: Diagnostic {
                message: text.into(),
                ..Diagnostic::default()
            },
        }
    }

    #[test]
    fn github_actions_line_with_column() {
        let issue = sample(
            "errcheck",
            "unchecked error",
            "app.js",
            10,
            15,
            "",
        );
        assert_eq!(
            format_issue_as_github(&issue),
            "::error file=app.js,line=10,col=15::unchecked error (errcheck)"
        );
    }

    #[test]
    fn github_actions_omits_col_when_zero() {
        let issue = sample("govet", "something", "a.go", 3, 0, "warning");
        assert_eq!(
            format_issue_as_github(&issue),
            "::warning file=a.go,line=3::something (govet)"
        );
    }

    #[test]
    fn github_actions_normalizes_backslashes() {
        let issue = sample("a", "msg", r"path\to\file.go", 1, 2, "");
        let s = format_issue_as_github(&issue);
        assert!(s.contains("file=path/to/file.go"), "{s}");
        assert!(!s.contains('\\'));
    }
}
