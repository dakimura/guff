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
//! - [`funlen`]
//! - [`gocyclo`]
//! - [`lll`]
//! - [`gocognit`]
//! - [`nestif`]
//! - [`cyclop`]
//!
//! DEFERRED (see DEVELOPMENT.md R14): remaining style bundle
//! (whitespace, wsl, nlreturn, …) and per-linter settings for the above.

mod asciicheck;
mod copyloopvar;
mod cyclop;
mod dogsled;
mod funlen;
mod gocognit;
mod goconst;
mod gocyclo;
mod goprintffuncname;
mod lll;
mod nestif;
mod perfsprint;
mod usestdlibvars;
mod usetesting;

pub use asciicheck::analyzer as asciicheck;
pub use copyloopvar::analyzer as copyloopvar;
pub use cyclop::analyzer as cyclop;
pub use dogsled::analyzer as dogsled;
pub use funlen::analyzer as funlen;
pub use gocognit::analyzer as gocognit;
pub use goconst::analyzer as goconst;
pub use gocyclo::analyzer as gocyclo;
pub use goprintffuncname::analyzer as goprintffuncname;
pub use lll::analyzer as lll;
pub use nestif::analyzer as nestif;
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
        funlen(),
        gocyclo(),
        lll(),
        gocognit(),
        nestif(),
        cyclop(),
    ]
}
