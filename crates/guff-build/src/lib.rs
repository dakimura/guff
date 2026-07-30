//! guff-build — a Rust port of Go's `go/build` package.
//!
//! Provides [`Context`], the supporting context for locating and classifying
//! Go package source files.
//!
//! Original Go source:
//!   Copyright 2011 The Go Authors. All rights reserved.
//!   Use of this source code is governed by a BSD-style license.

mod context;
pub mod go_source;
mod import_dir;
mod import_path;
mod match_file;
mod module;
mod package;

pub use context::{default_context, release_tags_for_version, Context, DEFAULT};
pub use import_path::is_local_import;
pub use match_file::MatchError;
pub use module::{
    find_module_root, module_import_dir, parse_mod_contents, parse_mod_file, ModFile, Replace,
    Require,
};
pub use package::{
    BuildError, ImportMode, MultiplePackageError, NoGoError, Package,
};
