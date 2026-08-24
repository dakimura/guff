//! guff-context — ports of context-related go/analysis linters.
//!
//! - [`noctx`] — AST-based (upstream uses buildssa; we match call names)
//! - [`fatcontext`] — nested context reassignment in loops / func lits
//! - [`contextcheck`] — SSA + package facts (intra-package; cross-pkg DEFERRED)
//! - [`sqlclosecheck`] — AST approximation (upstream uses buildssa; defer-only)

mod bodyclose;
mod contextcheck;
mod fatcontext;
mod noctx;
mod sqlclosecheck;

pub use bodyclose::analyzer as bodyclose;
pub use bodyclose::BodycloseOptions;
pub use contextcheck::analyzer as contextcheck;
pub use fatcontext::analyzer as fatcontext;
pub use fatcontext::FatcontextOptions;
pub use noctx::analyzer as noctx;
pub use sqlclosecheck::analyzer as sqlclosecheck;

use guff_analysis::Analyzer;

pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![noctx(), fatcontext(), bodyclose(), sqlclosecheck(), contextcheck()]
}
