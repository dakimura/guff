//! guff-packages — a Rust port of `golang.org/x/tools/go/packages`.
//!
//! Provides package loading via `go list -json` (default) with a pure-Rust
//! offline fallback when the `go` binary is unavailable, matching the data
//! model expected by `go/analysis` runners and golangci-lint.
//!
//! Original Go source:
//!   Copyright 2018 The Go Authors. All rights reserved.
//!   Use of this source code is governed by a BSD-style license.

mod config;
mod hash;
mod debug;
mod dedup;
mod driver;
mod golist;
mod load;
mod load_mode;
mod native;
mod native_cache;
mod offline;
mod package;
mod preset;
mod rss;
mod seed_cache;
mod speculate;
mod typecheck;

pub use config::Config;
pub use dedup::{filter_duplicate_packages, filter_test_main_packages, package_for_import_path};
pub use driver::{default_driver, offline_only_driver, AutoDriver, Driver, GoListDriver};
pub use golist::{
    detect_go_version_string, go_available, go_list_driver, normalize_pattern,
    stdlib_export_requested, stdlib_from_source_env, GoListError,
};
pub use load::{load, load_graph, load_graph_with_driver, load_with_driver, LoadError};
pub use load_mode::LoadMode;
pub use native::{diff_responses, native_or_golist, NativeListMode};
pub use offline::{offline_driver, OfflineDriver};
pub use package::{
    DriverResponse, Error, ErrorKind, Module, ModuleError, Package, TypecheckArtifacts,
};
pub use preset::load_for_go_analysis;
/// Fast hashmap used by package-loading hot paths (and a few public typecheck helpers).
pub use rustc_hash::FxHashMap;
pub use rss::{attribute_packages, enabled as rss_enabled, report_packages, PackageRssReport};
pub use speculate::{start_seed_speculation, SpeculativeSeed, SpeculativeSeedJob};
pub use typecheck::{
    needs_typecheck, typecheck_package, typecheck_package_with_seed, typecheck_packages,
    typecheck_roots, typecheck_roots_with_prebuilt_seed, TypecheckEnv,
};
