//! guff-types-errors — a Rust port of Go's `internal/types/errors` package.
//!
//! Defines the [`Code`] enum: identifiers for errors that can be produced
//! during type-checking. Used by the type checker to allow special-casing of
//! certain kinds of errors.
//!
//! Original Go source:
//!   Copyright 2020 The Go Authors. All rights reserved.
//!   Use of this source code is governed by a BSD-style license.

mod codes;
mod code_display;

pub use codes::Code;
