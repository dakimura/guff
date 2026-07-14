//! JSON output format — golangci-lint `pkg/printers/json.go` schema.
//!
//! Top level: `{"Issues":[...],"Report":...}`.
//! Each issue: `FromLinter`, `Text`, `Severity`, `SourceLines`, `Pos`, 
//! `ExpectNoLint`, `ExpectedNoLintLinter` (plus optional `LineRange` / `SuggestedFixes`).

use std::io::{self, Write};

use serde::Serialize;

use crate::exclude::Issue;

use super::Formatter;

/// golangci-lint JSON printer (`NewJSON` + `JSONResult`).
#[derive(Debug, Default, Clone)]
pub struct JsonFormatter {
    /// Optional report blob (`Report` key). `None` → JSON `null`.
    pub report: Option<JsonReport>,
}

impl JsonFormatter {
    pub fn new() -> Self {
        Self { report: None }
    }
}

impl Formatter for JsonFormatter {
    fn name(&self) -> &'static str {
        "json"
    }

    fn print(&self, issues: &[Issue], w: &mut dyn Write) -> io::Result<()> {
        let result = JsonResult {
            issues: issues.iter().map(JsonIssue::from).collect(),
            report: self.report.clone(),
        };
        serde_json::to_writer(&mut *w, &result).map_err(io::Error::other)?;
        // golangci's `json.NewEncoder(...).Encode` always appends a trailing newline.
        writeln!(w)?;
        Ok(())
    }
}

/// Top-level envelope matching `printers.JSONResult`.
#[derive(Debug, Serialize)]
struct JsonResult {
    #[serde(rename = "Issues")]
    issues: Vec<JsonIssue>,
    #[serde(rename = "Report")]
    report: Option<JsonReport>,
}

/// Subset of golangci `report.Data` we currently emit (warnings / error string).
///
/// Linter listing in Report is unused for now; emit empty omitable fields only
/// when a non-null Report is attached (tests pass `Report: null`).
#[derive(Debug, Clone, Default, Serialize)]
pub struct JsonReport {
    #[serde(rename = "Warnings", skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<JsonWarning>,
    #[serde(rename = "Error", skip_serializing_if = "String::is_empty")]
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonWarning {
    #[serde(rename = "Tag", skip_serializing_if = "String::is_empty")]
    pub tag: String,
    #[serde(rename = "Text")]
    pub text: String,
}

#[derive(Debug, Serialize)]
struct JsonIssue {
    #[serde(rename = "FromLinter")]
    from_linter: String,
    #[serde(rename = "Text")]
    text: String,
    #[serde(rename = "Severity")]
    severity: String,
    /// `null` when the source line was not captured (matches golangci nil slice).
    #[serde(rename = "SourceLines")]
    source_lines: Option<Vec<String>>,
    #[serde(rename = "Pos")]
    pos: JsonPos,
    #[serde(rename = "LineRange", skip_serializing_if = "Option::is_none")]
    line_range: Option<JsonLineRange>,
    #[serde(rename = "ExpectNoLint")]
    expect_no_lint: bool,
    #[serde(rename = "ExpectedNoLintLinter")]
    expected_no_lint_linter: String,
}

#[derive(Debug, Serialize)]
struct JsonPos {
    #[serde(rename = "Filename")]
    filename: String,
    #[serde(rename = "Offset")]
    offset: i64,
    #[serde(rename = "Line")]
    line: i64,
    #[serde(rename = "Column")]
    column: i64,
}

#[derive(Debug, Serialize)]
struct JsonLineRange {
    #[serde(rename = "From")]
    from: i64,
    #[serde(rename = "To")]
    to: i64,
}

impl From<&Issue> for JsonIssue {
    fn from(issue: &Issue) -> Self {
        let source_lines = issue
            .source_line
            .as_ref()
            .map(|line| vec![line.clone()]);
        // `Diagnostic.pos` is a guff token position (byte offset into the file set).
        // golangci's `token.Position.Offset` is the file-local byte offset; we reuse
        // the diagnostic pos when available (0 if unknown).
        let offset = i64::from(issue.diagnostic.pos);
        Self {
            from_linter: issue.from_linter.clone(),
            text: issue.text.clone(),
            severity: issue.severity.clone(),
            source_lines,
            pos: JsonPos {
                filename: issue.filename.clone(),
                offset,
                line: issue.line,
                column: issue.column,
            },
            line_range: None,
            // nolintlint ExpectNoLint plumbing is unused for ordinary issues.
            expect_no_lint: false,
            expected_no_lint_linter: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::Diagnostic;

    fn sample(from: &str, text: &str, file: &str, line: i64, col: i64, offset: u32) -> Issue {
        Issue {
            from_linter: from.into(),
            analyzer: from.into(),
            text: text.into(),
            severity: "warning".into(),
            filename: file.into(),
            line,
            column: col,
            source_line: None,
            diagnostic: Diagnostic {
                pos: offset,
                message: text.into(),
                ..Diagnostic::default()
            },
        }
    }

    #[test]
    fn json_matches_golangci_key_structure() {
        let issues = vec![
            sample("linter-a", "some issue", "path/to/filea.go", 10, 4, 2),
            {
                let mut i = sample(
                    "linter-b",
                    "another issue",
                    "path/to/fileb.go",
                    300,
                    9,
                    5,
                );
                i.severity = "error".into();
                i.source_line = Some("func foo() {".into());
                i
            },
        ];

        let mut buf = Vec::new();
        JsonFormatter::new().print(&issues, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();

        // Trailing newline from Encode.
        assert!(s.ends_with('\n'), "{s:?}");

        let v: serde_json::Value = serde_json::from_str(s.trim_end()).unwrap();
        assert!(v.get("Issues").unwrap().is_array());
        assert!(v.get("Report").unwrap().is_null());

        let first = &v["Issues"][0];
        assert_eq!(first["FromLinter"], "linter-a");
        assert_eq!(first["Text"], "some issue");
        assert_eq!(first["Severity"], "warning");
        assert!(first["SourceLines"].is_null());
        assert_eq!(first["Pos"]["Filename"], "path/to/filea.go");
        assert_eq!(first["Pos"]["Offset"], 2);
        assert_eq!(first["Pos"]["Line"], 10);
        assert_eq!(first["Pos"]["Column"], 4);
        assert_eq!(first["ExpectNoLint"], false);
        assert_eq!(first["ExpectedNoLintLinter"], "");

        let second = &v["Issues"][1];
        assert_eq!(second["FromLinter"], "linter-b");
        assert_eq!(second["SourceLines"], serde_json::json!(["func foo() {"]));
        assert_eq!(second["Pos"]["Line"], 300);
        assert_eq!(second["Severity"], "error");
    }

    #[test]
    fn empty_issues_emits_empty_array_not_null() {
        let mut buf = Vec::new();
        JsonFormatter::new().print(&[], &mut buf).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(v["Issues"], serde_json::json!([]));
        assert!(v["Report"].is_null());
    }
}
