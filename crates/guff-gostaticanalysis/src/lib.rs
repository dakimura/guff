//! guff-gostaticanalysis — ports of go/analysis linters in the
//! `gostaticanalysis` / related ecosystem.
//!
//! Currently registered as individual golangci-lint linter names:
//! - [`forcetypeassert`]
//! - [`nilnil`]
//! - [`makezero`]
//!
//! DEFERRED (need SSA / larger surface; see DEVELOPMENT.md R13):
//! nilerr, nilnesserr, mirror.

mod forcetypeassert;
mod makezero;
mod nilnil;

pub use forcetypeassert::analyzer as forcetypeassert;
pub use makezero::analyzer as makezero;
pub use nilnil::analyzer as nilnil;

use guff_analysis::Analyzer;

/// All analyzers in this crate (one per golangci linter name).
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![forcetypeassert(), nilnil(), makezero()]
}
