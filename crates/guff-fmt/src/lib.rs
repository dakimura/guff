//! Go source formatters for `guff fmt` (golangci-lint `pkg/goformatters` equivalent).
//!
//! Implemented: **gofmt** / **gofumpt** / **goimports** / **gci** / **golines** /
//! **swaggo**. Native ports under [`native`] (PERF_TASKS Task 1).

mod fmt_cache;
mod gci;
mod generated;
mod gofmt;
mod gofumpt;
mod goimports;
mod golines;
mod meta;
pub mod native;
mod runner;
mod swaggo;
mod timing;

pub use fmt_cache::{
    content_hash, fingerprint_parts, format_cache_dir_from_env, CachedCheck, FormatCheckCache,
    FMT_CHECK_SCHEMA,
};
pub use gci::{Gci, GciOptions};
pub use generated::{is_generated, GeneratedMode};
pub use gofmt::{Gofmt, GofmtOptions, RewriteRule};
pub use gofumpt::{Gofumpt, GofumptOptions};
pub use goimports::{Goimports, GoimportsOptions};
pub use golines::{Golines, GolinesOptions};
pub use meta::{is_formatter, MetaFormatter, KNOWN_FORMATTERS};
pub use native::{NativeKind, NativeOptions, SharedSkipObject};
pub use runner::{
    check_files_multi, AttributedFinding, FormatError, FormatFinding, Runner, RunnerOptions,
    RunStats,
};
pub use swaggo::Swaggo;
pub use timing::report as format_stage_report;

/// A source formatter: rewrite Go source bytes.
pub trait Formatter: Send + Sync {
    fn name(&self) -> &str;
    fn format(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError>;

    /// Stable fingerprint of options that affect formatting output.
    /// Used as part of the format-check cache key. Default: empty.
    fn options_fingerprint(&self) -> String {
        String::new()
    }

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

    /// Native options for B-10 shared skip-object parse, when this formatter
    /// participates as gci or gofumpt. Default: not participating.
    fn native_shared_skip_object(
        &self,
        _filename: &str,
    ) -> Option<native::SharedSkipObject> {
        None
    }
}
