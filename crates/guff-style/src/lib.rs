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
//! - [`gosmopolitan`]
//! - [`goheader`]
//! - [`gocheckcompilerdirectives`]
//! - [`forbidigo`]
//! - [`bidichk`]
//! - [`canonicalheader`]
//! - [`clickhouselint`]
//! - [`reassign`]
//! - [`recvcheck`]
//! - [`thelper`]
//! - [`iface`]
//! - [`interfacebloat`]
//! - [`embeddedstructfieldcheck`]
//! - [`gochecksumtype`]
//! - [`inamedparam`]
//! - [`arangolint`]
//! - [`containedctx`]
//! - [`decorder`]
//! - [`nonamedreturns`]
//! - [`noinlineerr`]
//! - [`testableexamples`]
//! - [`testpackage`]
//! - [`paralleltest`]
//! - [`protogetter`]
//! - [`tparallel`]
//! - [`intrange`]
//! - [`iotamixing`]
//! - [`grouper`]
//! - [`ireturn`]
//! - [`gosec`]
//! - [`funcorder`]
//! - [`tagliatelle`]
//! - [`goprintffuncname`]
//! - [`funlen`]
//! - [`gocyclo`]
//! - [`maintidx`]
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
//! - [`unparam`]
//! - [`unqueryvet`]
//! - [`promlinter`]
//! - [`ginkgolinter`]
//! - [`varnamelen`]
//! - [`wsl_v5`]
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
mod funcorder;
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
mod clickhouselint;
mod forbidigo;
mod gochecknoglobals;
mod gochecknoinits;
mod gosmopolitan;
mod goheader;
mod arangolint;
mod containedctx;
mod decorder;
mod embeddedstructfieldcheck;
mod gochecksumtype;
mod inamedparam;
mod interfacebloat;
mod noinlineerr;
mod nonamedreturns;
mod paralleltest;
mod protogetter;
mod testableexamples;
mod testpackage;
mod tparallel;
mod intrange;
mod iotamixing;
mod grouper;
mod ireturn;
mod gosec;
mod tagliatelle;
mod reassign;
mod recvcheck;
mod thelper;
mod gocognit;
mod goconst;
mod gocyclo;
mod maintidx;
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
mod unparam;
mod unqueryvet;
mod promlinter;
mod ginkgolinter;
mod unconvert;
mod usestdlibvars;
mod usetesting;
mod varnamelen;
mod whitespace;
mod wsl;
mod wsl_v5;

pub use options::{
    AsasalintOptions, BidichkOptions, CopyloopvarOptions, CyclopOptions, DecorderOptions,
    DogsledOptions, EmbeddedstructfieldcheckOptions, ExhaustiveOptions, ExhaustructOptions,
    ForbidigoOptions, ForbidigoPattern, FuncorderOptions, FunlenOptions, GochecksumtypeOptions,
    GocognitOptions, GoconstOptions, GocriticOptions, GocycloOptions, GoheaderOptions,
    GosmopolitanOptions,
    IfaceOptions, GosecOptions, GrouperOptions, InamedparamOptions, InterfacebloatOptions,
    IotamixingOptions, IreturnOptions, LllOptions, MaintidxOptions, LoggercheckOptions, MndOptions,
    ModernizeOptions, MusttagFunc, MusttagOptions, NakedretOptions, NestifOptions, NlreturnOptions,
    NonamedreturnsOptions, ParalleltestOptions, PerfsprintOptions, PreallocOptions,
    PredeclaredOptions, ReassignOptions, RecvcheckOptions, SloglintFunc, SloglintOptions,
    SuiteExtraAssertCallMode, TagalignOptions, TagliatelleOptions, TestifylintOptions,
    TestpackageOptions, ThelperKindOptions, ThelperOptions, UnconvertOptions, UnparamOptions,
    GinkgolinterOptions, PromlinterOptions, UnqueryvetOptions, UsestdlibvarsOptions,
    UsetestingOptions, VarnamelenOptions, WhitespaceOptions, WslOptions, WslV5Check, WslV5Options,
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
pub use funcorder::analyzer as funcorder;
pub use funlen::analyzer as funlen;
pub use gocheckcompilerdirectives::analyzer as gocheckcompilerdirectives;
pub use bidichk::analyzer as bidichk;
pub use canonicalheader::analyzer as canonicalheader;
pub use clickhouselint::analyzer as clickhouselint;
pub use forbidigo::analyzer as forbidigo;
pub use gochecknoglobals::analyzer as gochecknoglobals;
pub use gochecknoinits::analyzer as gochecknoinits;
pub use gosmopolitan::analyzer as gosmopolitan;
pub use goheader::analyzer as goheader;
pub use arangolint::analyzer as arangolint;
pub use containedctx::analyzer as containedctx;
pub use decorder::analyzer as decorder;
pub use embeddedstructfieldcheck::analyzer as embeddedstructfieldcheck;
pub use gochecksumtype::analyzer as gochecksumtype;
pub use inamedparam::analyzer as inamedparam;
pub use interfacebloat::analyzer as interfacebloat;
pub use noinlineerr::analyzer as noinlineerr;
pub use nonamedreturns::analyzer as nonamedreturns;
pub use paralleltest::analyzer as paralleltest;
pub use protogetter::analyzer as protogetter;
pub use testableexamples::analyzer as testableexamples;
pub use testpackage::analyzer as testpackage;
pub use tparallel::analyzer as tparallel;
pub use intrange::analyzer as intrange;
pub use iotamixing::analyzer as iotamixing;
pub use grouper::analyzer as grouper;
pub use ireturn::analyzer as ireturn;
pub use gosec::analyzer as gosec;
pub use tagliatelle::analyzer as tagliatelle;
pub use reassign::analyzer as reassign;
pub use recvcheck::analyzer as recvcheck;
pub use thelper::analyzer as thelper;
pub use gocognit::analyzer as gocognit;
pub use goconst::analyzer as goconst;
pub use gocritic::analyzer as gocritic;
pub use gocyclo::analyzer as gocyclo;
pub use maintidx::analyzer as maintidx;
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
pub use unparam::analyzer as unparam;
pub use unqueryvet::analyzer as unqueryvet;
pub use promlinter::analyzer as promlinter;
pub use ginkgolinter::analyzer as ginkgolinter;
pub use usestdlibvars::analyzer as usestdlibvars;
pub use usetesting::analyzer as usetesting;
pub use varnamelen::analyzer as varnamelen;
pub use whitespace::analyzer as whitespace;
pub use wsl::analyzer as wsl;
pub use wsl_v5::analyzer as wsl_v5;

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
        clickhouselint(),
        arangolint(),
        gochecknoinits(),
        gochecknoglobals(),
        gosmopolitan(),
        goheader(),
        gocheckcompilerdirectives(),
        forbidigo(),
        reassign(),
        recvcheck(),
        thelper(),
        iface(),
        interfacebloat(),
        embeddedstructfieldcheck(),
        gochecksumtype(),
        inamedparam(),
        containedctx(),
        decorder(),
        nonamedreturns(),
        noinlineerr(),
        paralleltest(),
        protogetter(),
        testableexamples(),
        testpackage(),
        tparallel(),
        intrange(),
        iotamixing(),
        grouper(),
        ireturn(),
        gosec(),
        tagliatelle(),
        goprintffuncname(),
        funlen(),
        gocyclo(),
        maintidx(),
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
        wsl_v5(),
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
        unparam(),
        unqueryvet(),
        promlinter(),
        ginkgolinter(),
        varnamelen(),
    ]
}
