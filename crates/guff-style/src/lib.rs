//! guff-style — ports of style / modernization go/analysis linters.
//!
//! Registered as individual golangci-lint linter names:
//! - [`copyloopvar`]
//! - [`usetesting`]
//! - [`usestdlibvars`]
//! - [`perfsprint`]
//! - [`goconst`]
//! - [`dogsled`]
//! - [`asciicheck`]
//! - [`goprintffuncname`]
//!
//! DEFERRED (see DEVELOPMENT.md R13 / R14): remaining style bundle
//! (funlen, gocyclo, …) and per-linter settings for the above.

mod asciicheck;
mod copyloopvar;
mod dogsled;
mod goconst;
mod goprintffuncname;
mod perfsprint;
mod usestdlibvars;
mod usetesting;

pub use asciicheck::analyzer as asciicheck;
pub use copyloopvar::analyzer as copyloopvar;
pub use dogsled::analyzer as dogsled;
pub use goconst::analyzer as goconst;
pub use goprintffuncname::analyzer as goprintffuncname;
pub use perfsprint::analyzer as perfsprint;
pub use usestdlibvars::analyzer as usestdlibvars;
pub use usetesting::analyzer as usetesting;

use guff_analysis::Analyzer;

/// All analyzers in this crate (one per golangci linter name).
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![
        copyloopvar(),
        usetesting(),
        usestdlibvars(),
        perfsprint(),
        goconst(),
        dogsled(),
        asciicheck(),
        goprintffuncname(),
    ]
}
