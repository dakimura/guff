//! guff-revive — port of [`github.com/mgechev/revive`](https://github.com/mgechev/revive)
//! (golangci-lint wrapper in `pkg/golinters/revive`).
//!
//! Registered as golangci-lint linter name [`revive`].
//!
//! This session implements the golint-default rule subset as individual Rust
//! rules. Full revive has 80+ rules with TOML configuration.
//!
//! DEFERRED (see DEVELOPMENT.md R14): `linters.settings.revive` YAML wiring
//! (per-rule enable/disable, arguments, severity, confidence); extended rules
//! (atomic, cyclomatic, struct-tag, …).

mod config;
mod failure;
mod ifelse;
mod names;
mod revive;
mod rules;
mod util;

pub use config::DEFAULT_RULES;
pub use revive::analyzer as revive;

use guff_analysis::Analyzer;

/// All analyzers in this crate.
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![revive()]
}
