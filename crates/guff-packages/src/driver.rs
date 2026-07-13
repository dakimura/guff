//! Package driver trait and default `go list` implementation.
//!
//! Port of `external.go` (`driver`, `defaultDriver`).

use crate::config::Config;
use crate::golist::go_list_driver;
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

/// Returns the built-in driver (`go list`).
pub fn default_driver() -> GoListDriver {
    GoListDriver
}
