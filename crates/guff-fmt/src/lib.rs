//! Go source formatters for `guff fmt` (golangci-lint `pkg/goformatters` equivalent).
//!
//! Implemented: **gofmt** / **gofumpt** / **goimports** / **gci** / **golines** /
//! **swaggo** (system binaries).

mod gci;
mod generated;
mod gofmt;
mod gofumpt;
mod goimports;
mod golines;
mod meta;
mod runner;
mod swaggo;

pub use gci::{Gci, GciOptions};
pub use generated::{is_generated, GeneratedMode};
pub use gofmt::{Gofmt, GofmtOptions, RewriteRule};
pub use gofumpt::{Gofumpt, GofumptOptions};
pub use goimports::{Goimports, GoimportsOptions};
pub use golines::{Golines, GolinesOptions};
pub use meta::{is_formatter, MetaFormatter, KNOWN_FORMATTERS};
pub use runner::{FormatError, FormatFinding, Runner, RunnerOptions, RunStats};
pub use swaggo::Swaggo;

/// A source formatter: rewrite Go source bytes.
pub trait Formatter: Send + Sync {
    fn name(&self) -> &str;
    fn format(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError>;
}
