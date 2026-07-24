//! guff-gostaticanalysis — ports of go/analysis linters in the
//! `gostaticanalysis` / related ecosystem.
//!
//! Currently registered as individual golangci-lint linter names:
//! - [`forcetypeassert`]
//! - [`nilnil`]
//! - [`makezero`]
//! - [`mirror`]
//! - [`nilnesserr`]
//! - [`nilerr`]

mod forcetypeassert;
mod makezero;
mod mirror;
mod nilerr;
mod nilnesserr;
mod nilnil;

pub use forcetypeassert::analyzer as forcetypeassert;
pub use makezero::analyzer as makezero;
pub use mirror::analyzer as mirror;
pub use nilerr::analyzer as nilerr;
pub use nilnesserr::analyzer as nilnesserr;
pub use nilnil::analyzer as nilnil;

use guff_analysis::Analyzer;

/// All analyzers in this crate (one per golangci linter name).
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![
        forcetypeassert(),
        nilnil(),
        makezero(),
        mirror(),
        nilnesserr(),
        nilerr(),
    ]
}
