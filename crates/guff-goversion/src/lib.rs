//! guff-goversion — a Rust port of Go's `internal/goversion` package.
//!
//! Exposes [`VERSION`], the Go 1.x version currently in development.

/// The Go 1.x version which is currently in development and will eventually
/// get released. It should be updated at the start of each development cycle
/// to be the version of the next Go 1.x release.
///
/// Equivalent to `internal/goversion.Version`.
pub const VERSION: u32 = 26;
