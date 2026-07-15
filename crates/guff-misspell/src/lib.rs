//! guff-misspell — port of [`github.com/golangci/misspell`](https://github.com/golangci/misspell)
//! (golangci-lint wrapper in `pkg/golinters/misspell`).
//!
//! Registered as golangci-lint linter name [`misspell`].
//!
//! DEFERRED (see DEVELOPMENT.md R14): `linters.settings.misspell` (locale /
//! ignore-words / extra-words / mode=restricted); locale UK.

mod case;
mod misspell;
mod notwords;
mod replacer;

pub use misspell::analyzer as misspell;
pub use replacer::{Diff, Replacer};

use guff_analysis::Analyzer;

/// All analyzers in this crate.
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![misspell()]
}
