//! Go standard-library ports that more than one guff linter crate needs.
//!
//! The rest of the `gostd` ports live in `guff-staticcheck`, because that is
//! the only crate that calls them: upstream staticcheck validates a constant
//! argument by *running the standard library on it* and reporting
//! `err.Error()` verbatim, so `SA1000`/`SA1001`/`SA1002`/`SA1007` need
//! `regexp`, `template`, `time` and `url` ported rather than approximated.
//!
//! `strconv` is here instead because a second crate needs it. `dupword`
//! unquotes a string literal, removes the duplicated word, and quotes the
//! result back — upstream does that with `strconv.Unquote` and `strconv.Quote`,
//! and an approximation would put approximate bytes into somebody's source
//! file. It reported without a fix for that reason alone
//! (COMPAT-HARDENING 続き 61), which is a linter silently doing less because of
//! where a file sits in the crate graph.
//!
//! The charter is exactly that: a port moves here when a second crate needs it,
//! not on the theory that it might. `regexp_table.rs` alone is 9,000 lines, and
//! every crate that depends on this one pays to build what it holds.

mod isprint_table;

pub mod strconv;
