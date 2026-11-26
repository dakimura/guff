//! Package driver trait and default `go list` / native / offline implementations.
//!
//! Port of `external.go` (`driver`, `defaultDriver`). Prefer order:
//! 1. [`crate::native`] when `GUFF_NATIVE_LIST` says so (C-3c)
//! 2. `go list` when `go` is on PATH
//! 3. [`OfflineDriver`] as last resort (main module + GOROOT only)

use crate::config::Config;
use crate::golist::{go_available, go_list_driver};
use crate::native::native_or_golist;
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

/// Driver that prefers native list / `go list` / offline per env and PATH.
#[derive(Debug, Default, Clone, Copy)]
pub struct AutoDriver;

impl Driver for AutoDriver {
    fn load(&self, cfg: &Config, patterns: &[String]) -> Result<DriverResponse, LoadError> {
        // `native_or_golist` honours GUFF_NATIVE_LIST and falls back appropriately.
        // When the mode is Off and go is missing it still tries native before
        // offline so external modules from GOMODCACHE work without `go`.
        match native_or_golist(cfg, patterns) {
            Ok(resp) => Ok(resp),
            Err(err) if !go_available() => {
                // Last resort: offline (no external modules).
                match offline_driver(cfg, patterns) {
                    Ok(resp) => Ok(resp),
                    Err(_) => Err(err),
                }
            }
            Err(err) => Err(err),
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
