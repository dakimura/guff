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

    /// Batch pre-filter for check mode: given real file paths, return the subset
    /// whose formatting differs (equivalent to `format(read(f)) != read(f)` for
    /// each `f`) using a single tool invocation per chunk — most formatter
    /// binaries expose a "list files that need formatting" mode (`gofmt -l`,
    /// `gci list`, …). The returned paths are the tool's own echoed forms (which
    /// may be path-cleaned, e.g. a leading `./` stripped); the caller maps them
    /// back to the original file paths.
    ///
    /// Returns `None` when this formatter has no batch mode, the batch spawn
    /// failed / a chunk exited non-zero (e.g. a parse error), or the configured
    /// options require per-file post-processing a list mode can't capture. The
    /// caller then falls back to per-file [`format`](Self::format) checks with
    /// identical behavior.
    fn list_unformatted(&self, _files: &[&std::path::Path]) -> Option<Vec<std::path::PathBuf>> {
        None
    }
}
