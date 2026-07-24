//! guff-analysis — a Rust port of `golang.org/x/tools/go/analysis`.
//!
//! Provides the `Analyzer` / `Pass` / `Diagnostic` framework expected by
//! golangci-lint-style runners.
//!
//! Original Go source:
//!   Copyright 2018 The Go Authors. All rights reserved.
//!   Use of this source code is governed by a BSD-style license.

mod analyzer;
pub mod callcheck;
pub mod code;
mod diagnostic;
mod fact_codec;
mod facts;
mod pass;
mod pattern_match;
pub mod passes;
mod settings;
mod ssa_util;
mod validate;

pub use passes::buildir::BuildIrResult;
pub use passes::facts::deprecated::{DeprecatedResult, IsDeprecated};
pub use passes::typeindex::Index as TypeIndex;
pub use pattern_match::{match_env, match_pattern, match_pos, matches};
pub use ssa_util::{
    append_modifies_param, block_control, closure_fn_in, dominates_all_returns, each_call,
    filter_debug, has_non_debug_referrer, is_call_to, is_call_to_any, is_in_loop, is_nil_const,
    param_value, referrers, short_call_name, store_modifies_param, terminates, walk_dominated,
};
pub use analyzer::{AnalysisResult, Analyzer, RunError, RunFn};
pub use diagnostic::{Diagnostic, RelatedInformation, SuggestedFix, TextEdit};
pub use passes::facts::generated::{GeneratedResult, Generator};
pub use fact_codec::{
    decode_fact, decode_facts_into, encode_fact_store, register_fact_decoder, remap_facts,
    EncodedFact,
};
pub use facts::{
    ensure_builtin_fact_decoders, Fact, FactStore, FactTypeId, ObjectFact, PackageFact, StringFact,
};
pub use pass::{Pass, PassInput};
pub use settings::SettingsBag;
pub use validate::{validate, ValidateError};
