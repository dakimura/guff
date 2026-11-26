//! Checkstyle XML output — golangci-lint `pkg/printers/checkstyle.go`.

use std::collections::BTreeMap;
use std::io::{self, Write};

use crate::exclude::Issue;

use super::severity::CHECKSTYLE;
use super::Formatter;

/// Checkstyle formatter (`version="5.0"`).
#[derive(Debug, Default, Clone)]
pub struct CheckstyleFormatter;

impl CheckstyleFormatter {
    pub fn new() -> Self {
        Self
    }
}

impl Formatter for CheckstyleFormatter {
    fn name(&self) -> &'static str {
        "checkstyle"
    }

    fn print(&self, issues: &[Issue], w: &mut dyn Write) -> io::Result<()> {
        // Group by file path; BTreeMap keeps files sorted (golangci `slices.SortedFunc`).
        let mut files: BTreeMap<&str, Vec<&Issue>> = BTreeMap::new();
        for issue in issues {
            files.entry(issue.filename.as_str()).or_default().push(issue);
        }

        writeln!(w, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
        writeln!(w)?;
        writeln!(w, r#"<checkstyle version="5.0">"#)?;
        for (name, file_issues) in files {
            writeln!(w, r#"  <file name="{}">"#, xml_escape_attr(name))?;
            for issue in file_issues {
                let severity = CHECKSTYLE.sanitize(&issue.severity);
                writeln!(
                    w,
                    r#"    <error column="{}" line="{}" message="{}" severity="{}" source="{}"></error>"#,
                    issue.column,
                    issue.line,
                    xml_escape_attr(&issue.text),
                    severity,
                    xml_escape_attr(&issue.from_linter),
                )?;
            }
            writeln!(w, "  </file>")?;
        }
        writeln!(w, "</checkstyle>")?;
        Ok(())
    }
}

fn xml_escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::Diagnostic;

    fn sample(
        from: &str,
        text: &str,
        file: &str,
        line: i64,
        col: i64,
        severity: &str,
    ) -> Issue {
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
    fn checkstyle_matches_golangci_fixture() {
        let issues = vec![
            sample("linter-a", "some issue", "path/to/filea.go", 10, 4, "warning"),
            sample("linter-b", "another issue", "path/to/fileb.go", 300, 9, "error"),
            sample("linter-c", "without severity", "path/to/filec.go", 300, 10, ""),
            sample("linter-d", "unknown severity", "path/to/filed.go", 300, 11, "foo"),
        ];

        let mut buf = Vec::new();
        CheckstyleFormatter::new().print(&issues, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();

        let expected = r#"<?xml version="1.0" encoding="UTF-8"?>

<checkstyle version="5.0">
  <file name="path/to/filea.go">
    <error column="4" line="10" message="some issue" severity="warning" source="linter-a"></error>
  </file>
  <file name="path/to/fileb.go">
    <error column="9" line="300" message="another issue" severity="error" source="linter-b"></error>
  </file>
  <file name="path/to/filec.go">
    <error column="10" line="300" message="without severity" severity="error" source="linter-c"></error>
  </file>
  <file name="path/to/filed.go">
    <error column="11" line="300" message="unknown severity" severity="error" source="linter-d"></error>
  </file>
</checkstyle>
"#;
        assert_eq!(s, expected);
    }

    #[test]
    fn empty_issues_emits_empty_checkstyle() {
        let mut buf = Vec::new();
        CheckstyleFormatter::new().print(&[], &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains(r#"<checkstyle version="5.0">"#));
        assert!(s.contains("</checkstyle>"));
        assert!(!s.contains("<file"));
    }
}
