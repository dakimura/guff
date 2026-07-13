//! guff-runner — parallel `go/analysis` driver over loaded packages.
//!
//! Wires `guff-packages::load` to `guff-analysis` analyzers with action-graph
//! scheduling similar to golangci-lint's `pkg/goanalysis` runner.
//!
//! Original Go reference:
//!   `golang.org/x/tools/go/analysis/checker`
//!   `github.com/golangci/golangci-lint/pkg/goanalysis`

mod action;
mod load_mode;
mod memory;
mod runner;

pub use action::{analyze, Action, Graph};
pub use load_mode::{
    ast_only_load_mode, infer_load_mode, load_mode_for_analyzers, types_load_mode,
    union_load_modes,
};
pub use memory::{trim_package_memory, trim_packages};
pub use runner::{run, run_on_packages, RunResult, RunnerError, RunnerOptions};
