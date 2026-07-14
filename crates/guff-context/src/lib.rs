//! guff-context — ports of context-related go/analysis linters.
//!
//! - [`noctx`] — AST-based (upstream uses buildssa; we match call names)
//! - [`fatcontext`] — nested context reassignment in loops / func lits

mod fatcontext;
mod noctx;

pub use fatcontext::analyzer as fatcontext;
pub use noctx::analyzer as noctx;

use guff_analysis::Analyzer;

pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![noctx(), fatcontext()]
}
