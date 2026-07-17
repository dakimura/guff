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
//! - [`asasalint`]
//! - [`gochecknoinits`]
//! - [`gochecknoglobals`]
//! - [`gocheckcompilerdirectives`]
//! - [`forbidigo`]
//! - [`bidichk`]
//! - [`canonicalheader`]
//! - [`reassign`]
//! - [`recvcheck`]
//! - [`thelper`]
//! - [`iface`]
//! - [`interfacebloat`]
//! - [`inamedparam`]
//! - [`containedctx`]
//! - [`decorder`]
//! - [`nonamedreturns`]
//! - [`testpackage`]
//! - [`paralleltest`]
//! - [`tparallel`]
//! - [`intrange`]
//! - [`iotamixing`]
//! - [`tagliatelle`]
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
//! - [`musttag`]
//! - [`loggercheck`]
//! - [`sloglint`]
//! - [`testifylint`]
//! - [`exptostd`]
//! - [`modernize`]
//! - [`gocritic`]
//!
//! DEFERRED (see DEVELOPMENT.md R14): remaining style bundle
//! (`guff-revive` / `guff-dupl`)
//! and per-linter settings / SuggestedFix for the above.

mod options;

mod asasalint;
mod asciicheck;
mod copyloopvar;
mod iface;
mod cyclop;
mod dogsled;
mod exhaustive;
mod exhaustruct;
mod exptostd;
mod funlen;
mod gocritic;
mod loggercheck;
mod modernize;
mod musttag;
mod sloglint;
mod testifylint;
mod gocheckcompilerdirectives;
mod bidichk;
mod canonicalheader;
mod forbidigo;
mod gochecknoglobals;
mod gochecknoinits;
mod containedctx;
mod decorder;
mod inamedparam;
mod interfacebloat;
mod nonamedreturns;
mod paralleltest;
mod testpackage;
mod tparallel;
mod intrange;
mod iotamixing;
mod tagliatelle;
mod reassign;
mod recvcheck;
mod thelper;
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
    AsasalintOptions, BidichkOptions, CopyloopvarOptions, CyclopOptions, DecorderOptions,
    DogsledOptions, ExhaustiveOptions, ExhaustructOptions, ForbidigoOptions, ForbidigoPattern,
    FunlenOptions, GocognitOptions, GoconstOptions, GocriticOptions, GocycloOptions, IfaceOptions,
    InamedparamOptions, InterfacebloatOptions, IotamixingOptions, LllOptions, LoggercheckOptions,
    MndOptions,
    ModernizeOptions, MusttagFunc, MusttagOptions, NakedretOptions, NestifOptions, NlreturnOptions,
    NonamedreturnsOptions, ParalleltestOptions, PerfsprintOptions, PreallocOptions,
    PredeclaredOptions, ReassignOptions, RecvcheckOptions, SloglintFunc, SloglintOptions,
    SuiteExtraAssertCallMode, TagalignOptions, TagliatelleOptions, TestifylintOptions,
    TestpackageOptions, ThelperKindOptions, ThelperOptions, UnconvertOptions, UsestdlibvarsOptions,
    UsetestingOptions, WhitespaceOptions, WslOptions,
};
pub use asasalint::analyzer as asasalint;
pub use asciicheck::analyzer as asciicheck;
pub use copyloopvar::analyzer as copyloopvar;
pub use iface::analyzer as iface;
pub use cyclop::analyzer as cyclop;
pub use dogsled::analyzer as dogsled;
pub use exhaustive::analyzer as exhaustive;
pub use exhaustruct::analyzer as exhaustruct;
pub use exptostd::analyzer as exptostd;
pub use funlen::analyzer as funlen;
pub use gocheckcompilerdirectives::analyzer as gocheckcompilerdirectives;
pub use bidichk::analyzer as bidichk;
pub use canonicalheader::analyzer as canonicalheader;
pub use forbidigo::analyzer as forbidigo;
pub use gochecknoglobals::analyzer as gochecknoglobals;
pub use gochecknoinits::analyzer as gochecknoinits;
pub use containedctx::analyzer as containedctx;
pub use decorder::analyzer as decorder;
pub use inamedparam::analyzer as inamedparam;
pub use interfacebloat::analyzer as interfacebloat;
pub use nonamedreturns::analyzer as nonamedreturns;
pub use paralleltest::analyzer as paralleltest;
pub use testpackage::analyzer as testpackage;
pub use tparallel::analyzer as tparallel;
pub use intrange::analyzer as intrange;
pub use iotamixing::analyzer as iotamixing;
pub use tagliatelle::analyzer as tagliatelle;
pub use reassign::analyzer as reassign;
pub use recvcheck::analyzer as recvcheck;
pub use thelper::analyzer as thelper;
pub use gocognit::analyzer as gocognit;
pub use goconst::analyzer as goconst;
pub use gocritic::analyzer as gocritic;
pub use gocyclo::analyzer as gocyclo;
pub use goprintffuncname::analyzer as goprintffuncname;
pub use lll::analyzer as lll;
pub use loggercheck::analyzer as loggercheck;
pub use mnd::analyzer as mnd;
pub use modernize::analyzer as modernize;
pub use musttag::analyzer as musttag;
pub use sloglint::analyzer as sloglint;
pub use testifylint::analyzer as testifylint;
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
        asasalint(),
        bidichk(),
        canonicalheader(),
        gochecknoinits(),
        gochecknoglobals(),
        gocheckcompilerdirectives(),
        forbidigo(),
        reassign(),
        recvcheck(),
        thelper(),
        iface(),
        interfacebloat(),
        inamedparam(),
        containedctx(),
        decorder(),
        nonamedreturns(),
        paralleltest(),
        testpackage(),
        tparallel(),
        intrange(),
        iotamixing(),
        tagliatelle(),
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
        musttag(),
        loggercheck(),
        sloglint(),
        testifylint(),
        exptostd(),
        modernize(),
        gocritic(),
    ]
}
