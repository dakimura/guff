//! Reasons the native lister cannot handle a request (fall back to `go list`).

use std::fmt;

/// Native list refused the request; callers should fall back to `go list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bail {
    pub reason: BailReason,
    pub detail: String,
}

impl Bail {
    pub fn new(reason: BailReason, detail: impl Into<String>) -> Self {
        Self {
            reason,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for Bail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "native list bail ({:?}): {}", self.reason, self.detail)
    }
}

impl std::error::Error for Bail {}

/// Why the native lister bailed (C-3c support matrix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BailReason {
    /// No `go.mod` at or above `dir`.
    NoGoMod,
    /// `go` directive older than 1.17 (lazy module graph pruning required).
    GoVersionTooOld,
    /// `go.work` present but the cwd is not inside any `use` module.
    GoWork,
    /// `vendor/` directory present (v2).
    Vendor,
    /// `exclude` / `retract` in go.mod (conservative).
    ExcludeOrRetract,
    /// Unsupported pattern (only `.` / `./...` / abs / main-module paths).
    UnsupportedPattern,
    /// Formerly used when `Config.tests` was unsupported; kept for ABI stability.
    Tests,
    /// Build flags other than `-tags=...`.
    UnsupportedBuildFlags,
    /// A required module is not extracted under GOMODCACHE (never download).
    ModuleNotInCache,
    /// Package imports `"C"` / has cgo files (C-3e delegates to `go list`).
    HasCgo,
    /// Import path could not be resolved.
    UnresolvedImport,
    /// Filesystem / parse error while listing.
    Io,
}
