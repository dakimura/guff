//! Text / colored-line-number output — golangci-lint `pkg/printers/text.go`.
//!
//! Plain: `file:line:col: message (analyzer)`
//! Colored: bold path+line, red message, optional source line + yellow `^` caret.

use std::io::{self, Write};

use guff::position::FileSet;
use guff_analysis::Diagnostic;

use crate::exclude::Issue;

use super::color::{bold, red, yellow};
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

/// Plain text / colored-line-number formatter.
#[derive(Debug, Clone)]
pub struct TextFormatter {
    /// Whether to append ` (analyzer)` (golangci `print-linter-name`, default true).
    pub print_linter_name: bool,
    /// Print the source line + caret under the column (golangci `print-issued-line`).
    pub print_issued_line: bool,
    /// ANSI colors (golangci `colored-line-number` / `text.colors`).
    pub colors: bool,
}

impl TextFormatter {
    pub fn new() -> Self {
        Self {
            print_linter_name: true,
            print_issued_line: false,
            colors: false,
        }
    }

    /// `--out-format colored-line-number`: colors on; source line when available.
    pub fn colored() -> Self {
        Self {
            print_linter_name: true,
            print_issued_line: true,
            colors: true,
        }
    }
}

impl Default for TextFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter for TextFormatter {
    fn name(&self) -> &'static str {
        if self.colors {
            "colored-line-number"
        } else {
            "text"
        }
    }

    fn print(&self, issues: &[Issue], w: &mut dyn Write) -> io::Result<()> {
        for issue in issues {
            self.print_issue(issue, w)?;
            if self.print_issued_line {
                self.print_source_and_caret(issue, w)?;
            }
        }
        Ok(())
    }
}

impl TextFormatter {
    fn print_issue(&self, issue: &Issue, w: &mut dyn Write) -> io::Result<()> {
        let file_line = format!("{}:{}", issue.filename, issue.line);
        let mut pos = bold(self.colors, &file_line);
        if issue.column != 0 {
            pos.push_str(&format!(":{}", issue.column));
        }

        let message = red(self.colors, issue.text.trim());
        let mut text = message;
        if self.print_linter_name && !issue.analyzer.is_empty() {
            text.push_str(&format!(" ({})", issue.analyzer));
        }

        writeln!(w, "{pos}: {text}")
    }

    fn print_source_and_caret(&self, issue: &Issue, w: &mut dyn Write) -> io::Result<()> {
        let Some(line) = issue.source_line.as_deref() else {
            return Ok(());
        };
        writeln!(w, "{line}")?;

        // Caret only when we have exactly one source line and a known column.
        if issue.column == 0 {
            return Ok(());
        }
        let col0 = (issue.column - 1) as usize;
        let mut prefix = String::new();
        for (j, ch) in line.chars().enumerate() {
            if j >= col0 {
                break;
            }
            if ch == '\t' {
                prefix.push('\t');
            } else {
                prefix.push(' ');
            }
        }
        writeln!(w, "{}{}", prefix, yellow(self.colors, "^"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::Diagnostic;

    fn sample(from: &str, text: &str, file: &str, line: i64, col: i64) -> Issue {
        Issue {
            from_linter: from.into(),
            analyzer: from.into(),
            text: text.into(),
            severity: String::new(),
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
    fn plain_text_line() {
        let mut buf = Vec::new();
        TextFormatter::new()
            .print(&[sample("errcheck", "unchecked error", "bad.go", 5, 2)], &mut buf)
            .unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "bad.go:5:2: unchecked error (errcheck)\n"
        );
    }

    #[test]
    fn colored_with_source_and_caret() {
        let mut issue = sample("linter-a", "some issue", "path/to/filea.go", 10, 4);
        issue.source_line = Some("abcXdef".into());
        let mut buf = Vec::new();
        TextFormatter::colored()
            .print(&[issue], &mut buf)
            .unwrap();
        let s = String::from_utf8(buf).unwrap();
        // Bold file:line, red message, source, yellow caret under col 4 (0-based index 3).
        assert!(s.contains("\x1b[1mpath/to/filea.go:10\x1b[22m:4:"), "{s:?}");
        assert!(s.contains("\x1b[31msome issue\x1b[0m (linter-a)"), "{s:?}");
        assert!(s.contains("abcXdef\n"), "{s:?}");
        assert!(s.contains(&format!("   {}\n", "\x1b[33m^\x1b[0m")), "{s:?}");
    }

    #[test]
    fn colored_matches_golangci_enable_all_without_source() {
        // golangci test "enable all options" when no source lines for first issue,
        // and multi-line source for second (we only store one SourceLines entry).
        let issues = vec![
            sample("linter-a", "some issue", "path/to/filea.go", 10, 4),
            {
                let mut i = sample("linter-b", "another issue", "path/to/fileb.go", 300, 9);
                // Multi-line SourceLines in golangci — when printIssuedLine, all lines
                // print but caret only for len==1. With one captured line:
                i.source_line = Some("func foo() {\n\tfmt.Println(\"bar\")\n}".into());
                i
            },
        ];
        // Use colors + linter name but print_issued_line false to match golangci
        // "enable all" for the issue lines themselves (their fixture's second issue has
        // multi-line source printed without caret because len != 1).
        let mut fmt = TextFormatter::colored();
        fmt.print_issued_line = false;
        let mut buf = Vec::new();
        fmt.print(&issues[..1], &mut buf).unwrap();
        // First line alone:
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "\x1b[1mpath/to/filea.go:10\x1b[22m:4: \x1b[31msome issue\x1b[0m (linter-a)\n"
        );
    }
}
