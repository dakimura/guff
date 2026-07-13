//! guff-pattern — AST pattern DSL for Staticcheck checks.
//!
//! Port of `honnef.co/go/tools/pattern`.

mod lexer;
mod parser;
mod pattern;
pub mod r#match;

pub use pattern::{must_parse, Parser, Pattern};
pub use r#match::{match_node, MatchEnv, Matcher, MatchValue};
