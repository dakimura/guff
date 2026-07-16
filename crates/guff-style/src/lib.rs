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
//! - [`prealloc`]
//! - [`tagalign`]
//! - [`wsl`]
//! - [`unconvert`]
//! - [`exhaustruct`]
//! - [`exhaustive`]
//!
//! DEFERRED (see DEVELOPMENT.md R14): remaining style bundle
//! (`guff-revive` / `guff-dupl`)
//! and per-linter settings / SuggestedFix for the above.

mod options;

mod asciicheck;
mod copyloopvar;
mod cyclop;
mod dogsled;
mod exhaustive;
mod exhaustruct;
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
mod prealloc;
mod predeclared;
mod tagalign;
mod unconvert;
mod usestdlibvars;
mod usetesting;
mod whitespace;
mod wsl;

pub use options::{
    CopyloopvarOptions, CyclopOptions, DogsledOptions, ExhaustiveOptions, ExhaustructOptions,
    FunlenOptions, GocognitOptions, GoconstOptions, GocycloOptions, LllOptions, MndOptions,
    NakedretOptions, NestifOptions, NlreturnOptions, PerfsprintOptions, PreallocOptions,
    PredeclaredOptions, TagalignOptions, UnconvertOptions, UsestdlibvarsOptions, UsetestingOptions,
    WhitespaceOptions, WslOptions,
};
pub use asciicheck::analyzer as asciicheck;
pub use copyloopvar::analyzer as copyloopvar;
pub use cyclop::analyzer as cyclop;
pub use dogsled::analyzer as dogsled;
pub use exhaustive::analyzer as exhaustive;
pub use exhaustruct::analyzer as exhaustruct;
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
pub use prealloc::analyzer as prealloc;
pub use predeclared::analyzer as predeclared;
pub use tagalign::analyzer as tagalign;
pub use unconvert::analyzer as unconvert;
pub use usestdlibvars::analyzer as usestdlibvars;
pub use usetesting::analyzer as usetesting;
pub use whitespace::analyzer as whitespace;
pub use wsl::analyzer as wsl;

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
        prealloc(),
        tagalign(),
        wsl(),
        unconvert(),
        exhaustruct(),
        exhaustive(),
    ]
}
