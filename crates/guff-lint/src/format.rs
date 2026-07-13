//! Diagnostic output formatting.

use guff::position::FileSet;
use guff_analysis::Diagnostic;

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
  if analyzer.is_empty() {
    format!("{loc}: {}", diag.message)
  } else {
    format!("{loc}: {} ({analyzer})", diag.message)
  }
}
