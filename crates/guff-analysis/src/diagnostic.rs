//! Diagnostics reported by analyzers.
//!
//! Port of `go/analysis/diagnostic.go`.

/// A message associated with a source location or range.
///
/// Equivalent to `analysis.Diagnostic`. All position values are interpreted
/// relative to [`super::Pass::fset`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Diagnostic {
    /// Start position (guff token position; `0` = unknown).
    pub pos: u32,
    /// Optional end position for range diagnostics.
    pub end: u32,
    /// Optional category string for classification / documentation lookup.
    pub category: String,
    /// Human-readable message.
    pub message: String,
    /// Optional link to additional documentation.
    pub url: String,
    /// Optional quick fixes for the diagnostic.
    pub suggested_fixes: Vec<SuggestedFix>,
    /// Optional secondary locations.
    pub related: Vec<RelatedInformation>,
}

/// A suggested code change for a diagnostic.
///
/// Port of `analysis.SuggestedFix`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestedFix {
    pub message: String,
    pub text_edits: Vec<TextEdit>,
}

/// A replacement of a byte range in a source file.
///
/// Port of `analysis.TextEdit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub pos: u32,
    pub end: u32,
    pub new_text: String,
}

/// Secondary position and message related to a primary diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedInformation {
    pub pos: u32,
    pub end: u32,
    pub message: String,
}
