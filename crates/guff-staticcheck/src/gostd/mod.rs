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

mod regexp_table;
mod unicode_table;

// `strconv` moved to `guff-gostd` so `dupword` in guff-comment can reach the
// Go-exact quote/unquote pair it needs to write a fix (COMPAT-HARDENING 続き
// 74). Re-exported under its old path: every `gostd::strconv::…` call site in
// this crate reads the same as before.
pub use guff_gostd::strconv;

pub mod fmt;
pub mod netip;
pub mod regexp;
pub mod template;
pub mod time;
pub mod unicode;
pub mod url;
