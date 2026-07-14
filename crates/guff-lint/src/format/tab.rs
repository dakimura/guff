//! Tab-separated output — golangci-lint `pkg/printers/tab.go`.

use std::io::{self, Write};

use crate::exclude::Issue;

use super::color::{bold, red};
use super::Formatter;

/// Tab formatter (`pos \\t [linter \\t] message`).
#[derive(Debug, Clone)]
pub struct TabFormatter {
    pub print_linter_name: bool,
    pub colors: bool,
}

impl TabFormatter {
    pub fn new() -> Self {
        Self {
            print_linter_name: true,
            colors: false,
        }
    }

    pub fn colored() -> Self {
        Self {
            print_linter_name: true,
            colors: true,
        }
    }
}

impl Default for TabFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter for TabFormatter {
    fn name(&self) -> &'static str {
        if self.colors {
            "colored-tab"
        } else {
            "tab"
        }
    }

    fn print(&self, issues: &[Issue], w: &mut dyn Write) -> io::Result<()> {
        // Collect rows, then align like Go's tabwriter (pad with spaces; width ignores ANSI).
        let mut rows: Vec<(String, Option<String>, String)> = Vec::with_capacity(issues.len());
        for issue in issues {
            // Bold only the `file:line` portion (matching fatih color.Bold on that prefix).
            let file_line = format!("{}:{}", issue.filename, issue.line);
            let colored_fl = bold(self.colors, &file_line);
            let pos = if issue.column != 0 {
                format!("{colored_fl}:{}", issue.column)
            } else {
                colored_fl
            };
            let linter = if self.print_linter_name {
                Some(issue.from_linter.clone())
            } else {
                None
            };
            let text = red(self.colors, &issue.text);
            rows.push((pos, linter, text));
        }

        // Widths from visible (strip ANSI) lengths so columns line up with colors on.
        let mut w0 = 0usize;
        let mut w1 = 0usize;
        for (pos, linter, _) in &rows {
            w0 = w0.max(visible_len(pos));
            if let Some(l) = linter {
                w1 = w1.max(l.len());
            }
        }

        for (pos, linter, text) in &rows {
            let pad0 = " ".repeat(w0.saturating_sub(visible_len(pos)));
            if let Some(l) = linter {
                let pad1 = " ".repeat(w1.saturating_sub(l.len()));
                // Go tabwriter minwidth 0, padding 2 → two spaces between columns.
                writeln!(w, "{pos}{pad0}  {l}{pad1}  {text}")?;
            } else {
                writeln!(w, "{pos}{pad0}  {text}")?;
            }
        }
        Ok(())
    }
}

fn visible_len(s: &str) -> usize {
    let mut len = 0usize;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        len += 1;
    }
    len
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
    fn tab_with_linter_name() {
        let issues = vec![
            sample("linter-a", "some issue", "path/to/filea.go", 10, 4),
            sample("linter-b", "another issue", "path/to/fileb.go", 300, 9),
        ];
        let mut buf = Vec::new();
        TabFormatter::new().print(&issues, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(
            s,
            "path/to/filea.go:10:4   linter-a  some issue\n\
             path/to/fileb.go:300:9  linter-b  another issue\n"
        );
    }

    #[test]
    fn tab_without_linter_name() {
        let issues = vec![
            sample("linter-a", "some issue", "path/to/filea.go", 10, 4),
            sample("linter-b", "another issue", "path/to/fileb.go", 300, 9),
        ];
        let mut fmt = TabFormatter::new();
        fmt.print_linter_name = false;
        let mut buf = Vec::new();
        fmt.print(&issues, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(
            s,
            "path/to/filea.go:10:4   some issue\n\
             path/to/fileb.go:300:9  another issue\n"
        );
    }

    #[test]
    fn tab_colored() {
        let issues = vec![sample("linter-a", "some issue", "path/to/filea.go", 10, 4)];
        let mut buf = Vec::new();
        TabFormatter::colored().print(&issues, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("\x1b[1m"), "expected bold: {s:?}");
        assert!(s.contains("\x1b[31m"), "expected red: {s:?}");
        assert!(s.contains("linter-a"));
    }
}
