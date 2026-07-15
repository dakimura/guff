//! guff-dupl — port of [`github.com/golangci/dupl`](https://github.com/golangci/dupl)
//! (golangci-lint wrapper in `pkg/golinters/dupl`).
//!
//! Registered as golangci-lint linter name [`dupl`].
//!
//! Reads [`Options`] from [`guff_analysis::SettingsBag`] when set via
//! `guff-lint` `linters.settings.dupl.threshold`.

mod dupl;
mod engine;
mod golang;
mod node_type;
mod suffixtree;
mod syntax;

pub use dupl::{analyzer as dupl, Options};
pub use engine::{run, CloneLoc, DuplIssue, DEFAULT_THRESHOLD};

use guff_analysis::Analyzer;

/// All analyzers in this crate.
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![dupl()]
}
