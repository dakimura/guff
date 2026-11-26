//! guff-error — ports of error-related go/analysis linters.
//!
//! Registered as individual golangci-lint linter names:
//! - [`errname`]
//! - [`err113`]
//! - [`durationcheck`]
//! - [`errorlint`]
//! - [`wrapcheck`]
//! - [`errchkjson`]
//! - [`rowserrcheck`]

mod durationcheck;
mod err113;
mod errchkjson;
mod errname;
mod errorlint;
mod rowserrcheck;
mod util;
mod wrapcheck;

pub use durationcheck::analyzer as durationcheck;
pub use err113::analyzer as err113;
pub use errchkjson::analyzer as errchkjson;
pub use errchkjson::ErrchkjsonOptions;
pub use errorlint::ErrorlintOptions;
pub use errname::analyzer as errname;
pub use errorlint::analyzer as errorlint;
pub use rowserrcheck::analyzer as rowserrcheck;
pub use rowserrcheck::RowserrcheckOptions;
pub use wrapcheck::analyzer as wrapcheck;
pub use wrapcheck::WrapcheckOptions;

use guff_analysis::Analyzer;

pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![
        errname(),
        err113(),
        durationcheck(),
        errorlint(),
        wrapcheck(),
        errchkjson(),
        rowserrcheck(),
    ]
}
