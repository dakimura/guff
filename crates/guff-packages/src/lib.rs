//! guff-packages — a Rust port of `golang.org/x/tools/go/packages`.
//!
//! Provides package loading via `go list -json`, matching the data model
//! expected by `go/analysis` runners and golangci-lint.
//!
//! Original Go source:
//!   Copyright 2018 The Go Authors. All rights reserved.
//!   Use of this source code is governed by a BSD-style license.

mod config;
mod dedup;
mod driver;
mod golist;
mod load;
mod load_mode;
mod package;
mod preset;
mod typecheck;

pub use config::Config;
pub use dedup::{filter_duplicate_packages, filter_test_main_packages};
pub use driver::{default_driver, Driver, GoListDriver};
pub use golist::{go_available, go_list_driver, normalize_pattern, GoListError};
pub use load::{load, load_graph, load_graph_with_driver, load_with_driver, LoadError};
pub use load_mode::LoadMode;
pub use package::{
    DriverResponse, Error, ErrorKind, Module, ModuleError, Package, TypecheckArtifacts,
};
pub use preset::load_for_go_analysis;
pub use typecheck::{
    needs_typecheck, typecheck_package, typecheck_packages, typecheck_roots, TypecheckEnv,
};
