//! guff-comment — ports of comment-related go/analysis linters.
//!
//! Registered as individual golangci-lint linter names:
//! - [`godot`]
//! - [`godox`]
//! - [`dupword`]
//! - [`godoclint`]
//!
//! `linters.settings` for all four are wired. DEFERRED (see DEVELOPMENT.md
//! R14): SuggestedFix, godot `toplevel`/`noinline` scopes, dupword cross-line
//! checks / `skip-raw-strings`, godoclint strict/extra rules and
//! `//godoclint:disable` directives.

mod dupword;
mod godoclint;
mod godot;
mod godox;
mod options;
mod util;

pub use dupword::analyzer as dupword;
pub use godoclint::analyzer as godoclint;
pub use godot::analyzer as godot;
pub use godox::analyzer as godox;
pub use options::{DupwordOptions, GodoclintOptions, GodotOptions, GodoxOptions};

use guff_analysis::Analyzer;

/// All analyzers in this crate (one per golangci linter name).
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![godot(), godox(), dupword(), godoclint()]
}
