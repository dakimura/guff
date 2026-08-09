//! Ports of Go standard-library parsers, used by the SA10xx checks.
//!
//! Upstream staticcheck validates a constant argument by *calling the standard
//! library on it* and reporting `err.Error()` verbatim — `time.Parse(s, s)` for
//! [`SA1002`](../sa1002/index.html), `url.Parse(s)` for `SA1007`,
//! `regexp.Compile(s)` for `SA1000`, `template.Parse(s)` for `SA1001`. An
//! approximation built on a Rust crate therefore diverges twice: it accepts a
//! different set of inputs (false positives / negatives) and it prints a
//! different message. The only way to match is to port the Go parser, so that
//! is what lives here.

mod isprint_table;

pub mod netip;
pub mod strconv;
pub mod time;
pub mod url;
