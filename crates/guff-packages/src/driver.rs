//! Package driver trait and default `go list` / offline implementations.
//!
//! Port of `external.go` (`driver`, `defaultDriver`). When `go` is on PATH the
//! default driver shells out to `go list -json`; otherwise it falls back to the
//! pure-Rust [`OfflineDriver`] (PL02).

use crate::config::Config;
use crate::golist::{go_available, go_list_driver};
use crate::offline::{offline_driver, OfflineDriver};
use crate::package::DriverResponse;
use crate::LoadError;

/// Loads package metadata for the given patterns.
///
/// Equivalent to `packages.driver`.
pub trait Driver {
    fn load(&self, cfg: &Config, patterns: &[String]) -> Result<DriverResponse, LoadError>;
}

/// Default driver that shells out to `go list -json`.
#[derive(Debug, Default, Clone, Copy)]
pub struct GoListDriver;

impl Driver for GoListDriver {
    fn load(&self, cfg: &Config, patterns: &[String]) -> Result<DriverResponse, LoadError> {
        go_list_driver(cfg, patterns).map_err(Into::into)
    }
}

/// Driver that prefers `go list`, falling back to [`OfflineDriver`] when `go`
/// is missing from PATH (CI sandboxes / offline environments).
#[derive(Debug, Default, Clone, Copy)]
pub struct AutoDriver;

impl Driver for AutoDriver {
    fn load(&self, cfg: &Config, patterns: &[String]) -> Result<DriverResponse, LoadError> {
        if go_available() {
            go_list_driver(cfg, patterns).map_err(Into::into)
        } else {
            offline_driver(cfg, patterns)
        }
    }
}

/// Returns the built-in auto driver (`go list` with offline fallback).
pub fn default_driver() -> AutoDriver {
    AutoDriver
}

/// Explicit offline-only driver (never invokes `go`).
pub fn offline_only_driver() -> OfflineDriver {
    OfflineDriver
}
