//! guff-style — ports of style / modernization go/analysis linters.
//!
//! Registered as individual golangci-lint linter names:
//! - [`copyloopvar`]
//! - [`usetesting`]
//! - [`usestdlibvars`]
//!
//! DEFERRED (see DEVELOPMENT.md R13 / R14): perfsprint, goconst, and the
//! rest of the style bundle (funlen, gocyclo, …).

mod copyloopvar;
mod usestdlibvars;
mod usetesting;

pub use copyloopvar::analyzer as copyloopvar;
pub use usestdlibvars::analyzer as usestdlibvars;
pub use usetesting::analyzer as usetesting;

use guff_analysis::Analyzer;

/// All analyzers in this crate (one per golangci linter name).
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![copyloopvar(), usetesting(), usestdlibvars()]
}
