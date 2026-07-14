//! guff-comment — ports of comment-related go/analysis linters.
//!
//! Registered as individual golangci-lint linter names:
//! - [`godot`]
//! - [`godox`]
//! - [`dupword`]
//!
//! DEFERRED (see DEVELOPMENT.md R14): settings wiring, SuggestedFix,
//! godot scope/capital/exclude, dupword keyword filters / cross-line
//! comment checks.

mod dupword;
mod godot;
mod godox;
mod util;

pub use dupword::analyzer as dupword;
pub use godot::analyzer as godot;
pub use godox::analyzer as godox;

use guff_analysis::Analyzer;

/// All analyzers in this crate (one per golangci linter name).
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![godot(), godox(), dupword()]
}
