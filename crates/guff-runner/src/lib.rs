//! guff-runner — parallel `go/analysis` driver over loaded packages.
//!
//! Wires `guff-packages::load` to `guff-analysis` analyzers with action-graph
//! scheduling similar to golangci-lint's `pkg/goanalysis` runner.
//!
//! Original Go reference:
//!   `golang.org/x/tools/go/analysis/checker`
//!   `github.com/golangci/golangci-lint/pkg/goanalysis`

mod action;
mod cache;
mod hash;
mod load_mode;
mod memory;
mod runner;

pub use action::{analyze, analyze_with_settings, Action, Graph};
pub use cache::{
    build_salt, cache_dir_size, clean_cache, default_cache_dir, default_go_cache_dir,
    detect_go_version, ensure_go_cache_env, is_under_go_cache, load_from_cache, save_to_cache,
    CacheError, CacheStats, CachedDiagnostic, HashMode, IssueCache, ENV_GOCACHE,
    ENV_GOLANGCI_LINT_CACHE, ENV_GUFF_CACHE,
};
pub use load_mode::{
    ast_only_load_mode, infer_load_mode, load_mode_for_analyzers, types_load_mode,
    union_load_modes,
};
pub use memory::{trim_package_memory, trim_packages};
pub use runner::{run, run_on_packages, RunResult, RunnerError, RunnerOptions};

/// Configures rayon's global thread pool with a larger per-worker stack. The
/// default (~512 KiB on macOS) is too small for deep hybrid dependency
/// type-checking and SSA construction; call once before any `par_iter` use.
pub fn init_rayon_global_stack() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        const STACK: usize = 8 * 1024 * 1024;
        let _ = rayon::ThreadPoolBuilder::new()
            .stack_size(STACK)
            .build_global();
    });
}
