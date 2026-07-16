//! guff-import — ports of import / go.mod go/analysis linters.
//!
//! Registered as individual golangci-lint linter names:
//! - [`depguard`]
//! - [`gomoddirectives`]
//! - [`gomodguard`]
//!
//! `linters.settings` for all three are wired (depguard rules / list-mode /
//! files / allow / deny; gomoddirectives option flags; gomodguard /
//! gomodguard_v2 blocked + local-replace). DEFERRED: depguard path
//! placeholders; gomoddirectives ignore/toolchain-pattern/go-version-pattern/
//! check-module-path; gomodguard allowed modules/domains / version constraints /
//! match-type.

mod depguard;
mod gomod;
mod gomoddirectives;
mod gomodguard;
mod options;

pub use depguard::analyzer as depguard;
pub use gomoddirectives::analyzer as gomoddirectives;
pub use gomodguard::analyzer as gomodguard;
pub use gomodguard::analyzer_block_logrus;
pub use gomodguard::analyzer_local_replace;
pub use options::{
    DenyEntry, DepguardOptions, DepguardRule, GomoddirectivesOptions, GomodguardOptions, ListMode,
};

use guff_analysis::Analyzer;

/// All analyzers in this crate (one per golangci linter name).
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![depguard(), gomoddirectives(), gomodguard()]
}
