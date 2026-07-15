//! guff-import — ports of import / go.mod go/analysis linters.
//!
//! Registered as individual golangci-lint linter names:
//! - [`depguard`]
//! - [`gomoddirectives`]
//! - [`gomodguard`]
//!
//! DEFERRED (see DEVELOPMENT.md R14): `linters.settings` wiring for all three
//! (depguard rules / list-mode / file globs; gomoddirectives option flags;
//! gomodguard allowed/blocked/version constraints / gomodguard_v2).

mod depguard;
mod gomod;
mod gomoddirectives;
mod gomodguard;

pub use depguard::analyzer as depguard;
pub use gomoddirectives::analyzer as gomoddirectives;
pub use gomodguard::analyzer as gomodguard;
pub use gomodguard::analyzer_block_logrus;
pub use gomodguard::analyzer_local_replace;

use guff_analysis::Analyzer;

/// All analyzers in this crate (one per golangci linter name).
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![depguard(), gomoddirectives(), gomodguard()]
}
