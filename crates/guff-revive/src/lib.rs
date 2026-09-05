//! guff-revive — port of [`github.com/mgechev/revive`](https://github.com/mgechev/revive)
//! (golangci-lint wrapper in `pkg/golinters/revive`).
//!
//! Registered as golangci-lint linter name [`revive`].
//!
//! This session implements the golint-default rule subset as individual Rust
//! rules. Full revive has 80+ rules with TOML configuration.

mod astfmt;
mod config;
mod directives;
mod failure;
pub mod filefilter;
mod ifelse;
mod names;
mod revive;
mod rules;
mod settings;
mod util;

pub use config::{DEFAULT_RULES, EXTENDED_RULES, extended_test_settings, with_extended_rules, with_settings};
pub use revive::analyzer as revive;
pub use settings::{RuleArgument, RuleSetting, Settings};

use guff_analysis::Analyzer;

/// All analyzers in this crate.
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![revive()]
}
