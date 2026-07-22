//! Port of Go's `go/printer` package.
//!
//! # STATUS (PERF_TASKS Task 1b) — 2026-07-22
//!
//! ## Done
//! - `text/tabwriter` port ([`crate::tabwriter`]) with golden tests vs Go
//! - Printer core (`printer.rs`): whitespace, comments, trimmer, Config/Fprint
//! - Node printer (`nodes.rs`): exprs/stmts/decls/files (mechanical port)
//! - `gobuild.rs`: `//go:build` / `// +build` relocation
//! - `format::source` wired; native `guff-fmt-native gofmt` path live
//! - **prometheus corpus: 725/725 byte-identical** (`fmt_diff.py --formatter gofmt --corpus prometheus`)
//!
//! ## TODO / known gaps
//! - `format_doc_comment` is a no-op (needs `go/doc/comment` Parser/Printer).
//!   Idempotent on already-gofmt'd input; may diverge on messy package docs.
//! - `gofmt -s` simplify not implemented.
//! - GOROOT corpus not yet fully green-gated (run `fmt_diff.py --corpus goroot`).
//! - Parser `expr_eq_shallow` for param grouping is structural (not Go's
//!   pointer identity after type distribution); extended for common type
//!   shapes including `IndexListExpr` generics.

mod comment;
pub(crate) mod gobuild;
mod nodes;
pub(crate) mod printer;

pub use printer::{
    fprint, CommentedNode, Config, Mode, PrintNode, NORMALIZE_NUMBERS, RAW_FORMAT, SOURCE_POS,
    TAB_INDENT, USE_SPACES,
};

pub(crate) use comment::format_doc_comment;
pub(crate) use printer::Printer;
