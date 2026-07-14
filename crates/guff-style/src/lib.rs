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
//! - [`nakedret`]
//! - [`nosprintfhostport`]
//! - [`predeclared`]
//! - [`whitespace`]
//! - [`nlreturn`]
//! - [`mnd`]
//!
//! DEFERRED (see DEVELOPMENT.md R14): remaining style bundle
//! (wsl, prealloc, tagalign, …) and per-linter settings for the above.

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
mod mnd;
mod nakedret;
mod nestif;
mod nlreturn;
mod nosprintfhostport;
mod perfsprint;
mod predeclared;
mod usestdlibvars;
mod usetesting;
mod whitespace;

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
pub use mnd::analyzer as mnd;
pub use nakedret::analyzer as nakedret;
pub use nestif::analyzer as nestif;
pub use nlreturn::analyzer as nlreturn;
pub use nosprintfhostport::analyzer as nosprintfhostport;
pub use perfsprint::analyzer as perfsprint;
pub use predeclared::analyzer as predeclared;
pub use usestdlibvars::analyzer as usestdlibvars;
pub use usetesting::analyzer as usetesting;
pub use whitespace::analyzer as whitespace;

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
        nakedret(),
        nosprintfhostport(),
        predeclared(),
        whitespace(),
        nlreturn(),
        mnd(),
    ]
}
