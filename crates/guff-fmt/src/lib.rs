//! Go source formatters for `guff fmt` (golangci-lint `pkg/goformatters` equivalent).
//!
//! First formatter: **gofmt** (invokes the system `gofmt` binary from the Go toolchain).
//! Remaining formatters (gofumpt / goimports / gci / golines / swaggo) are DEFERRED → R15.

mod gofmt;
mod meta;
mod runner;

pub use gofmt::{Gofmt, GofmtOptions, RewriteRule};
pub use meta::{is_formatter, MetaFormatter, KNOWN_FORMATTERS};
pub use runner::{FormatError, Runner, RunnerOptions, RunStats};

/// A source formatter: rewrite Go source bytes.
pub trait Formatter: Send + Sync {
    fn name(&self) -> &str;
    fn format(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError>;
}
