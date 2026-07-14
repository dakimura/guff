//! guff-error — ports of error-related go/analysis linters.
//!
//! Registered as individual golangci-lint linter names:
//! - [`errname`]
//! - [`err113`]
//! - [`durationcheck`]

mod durationcheck;
mod err113;
mod errname;
mod util;

pub use durationcheck::analyzer as durationcheck;
pub use err113::analyzer as err113;
pub use errname::analyzer as errname;

use guff_analysis::Analyzer;

pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![errname(), err113(), durationcheck()]
}
