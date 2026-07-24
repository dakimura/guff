//! guff-govet — Rust port of Go vet analysis passes.

mod assign;
mod atomic;
mod bools;
mod buildconstraint;
mod buildtag;
mod cgocall;
mod composites;
mod copylocks;
mod defers;
mod directive;
mod errorsas;
mod expreq;
mod framepointer;
mod govet_util;
mod httpresponse;
mod ifaceassert;
mod inline;
mod lockpath;
mod loopclosure;
mod lostcancel;
mod nilfunc;
mod printf;
mod shift;
mod sigchanyzer;
mod slog;
mod stdmethods;
mod stringintconv;
mod structtag;
mod testpass;
mod timeformat;
mod unmarshal;
mod unreachable;
mod unsafeptr;
mod unusedresult;

pub use assign::analyzer as assign_analyzer;
pub use atomic::analyzer as atomic_analyzer;
pub use bools::analyzer as bools_analyzer;
pub use buildtag::analyzer as buildtag_analyzer;
pub use cgocall::analyzer as cgocall_analyzer;
pub use composites::analyzer as composites_analyzer;
pub use copylocks::analyzer as copylocks_analyzer;
pub use defers::analyzer as defers_analyzer;
pub use directive::analyzer as directive_analyzer;
pub use errorsas::analyzer as errorsas_analyzer;
pub use framepointer::analyzer as framepointer_analyzer;
pub use httpresponse::analyzer as httpresponse_analyzer;
pub use ifaceassert::analyzer as ifaceassert_analyzer;
pub use inline::analyzer as inline_analyzer;
pub use loopclosure::analyzer as loopclosure_analyzer;
pub use lostcancel::analyzer as lostcancel_analyzer;
pub use nilfunc::analyzer as nilfunc_analyzer;
pub use printf::analyzer as printf_analyzer;
pub use shift::analyzer as shift_analyzer;
pub use sigchanyzer::analyzer as sigchanyzer_analyzer;
pub use slog::analyzer as slog_analyzer;
pub use stdmethods::analyzer as stdmethods_analyzer;
pub use stringintconv::analyzer as stringintconv_analyzer;
pub use structtag::analyzer as structtag_analyzer;
pub use testpass::analyzer as tests_analyzer;
pub use timeformat::analyzer as timeformat_analyzer;
pub use unmarshal::analyzer as unmarshal_analyzer;
pub use unreachable::analyzer as unreachable_analyzer;
pub use unsafeptr::analyzer as unsafeptr_analyzer;
pub use unusedresult::analyzer as unusedresult_analyzer;

use guff_analysis::Analyzer;

/// All govet analyzers implemented in this crate.
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![
        assign::analyzer(),
        atomic::analyzer(),
        bools::analyzer(),
        buildtag::analyzer(),
        cgocall::analyzer(),
        composites::analyzer(),
        copylocks::analyzer(),
        defers::analyzer(),
        directive::analyzer(),
        errorsas::analyzer(),
        framepointer::analyzer(),
        httpresponse::analyzer(),
        ifaceassert::analyzer(),
        inline::analyzer(),
        loopclosure::analyzer(),
        lostcancel::analyzer(),
        nilfunc::analyzer(),
        printf::analyzer(),
        shift::analyzer(),
        sigchanyzer::analyzer(),
        slog::analyzer(),
        stdmethods::analyzer(),
        stringintconv::analyzer(),
        structtag::analyzer(),
        testpass::analyzer(),
        timeformat::analyzer(),
        unmarshal::analyzer(),
        unreachable::analyzer(),
        unsafeptr::analyzer(),
        unusedresult::analyzer(),
    ]
}
