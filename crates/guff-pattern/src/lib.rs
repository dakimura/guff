//! guff-pattern — AST pattern DSL for Staticcheck checks.
//!
//! Port of `honnef.co/go/tools/pattern`.

mod lexer;
mod parser;
mod pattern;
pub mod r#match;

pub use pattern::{must_parse, IndexSymbol, Parser, Pattern};

/// Every node-kind name `Pattern::entry_kinds` can contain.
///
/// Exposed so callers that turn entry kinds into a node-kind bitset can
/// assert the whole vocabulary resolves, rather than discovering a
/// non-resolving name as a silently widened mask at run time.
pub fn all_entry_kinds() -> &'static [&'static str] {
    pattern::ALL_ENTRY_KINDS
}
pub use r#match::{match_node, MatchEnv, Matcher, MatchValue};
