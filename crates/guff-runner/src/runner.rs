//! High-level analysis runner.
//!
//! Port of `golangci-lint/pkg/goanalysis/runner.go`.

use crate::hash::HashMap;
use std::sync::Arc;

use guff_analysis::{Analyzer, Diagnostic, SettingsBag, ValidateError};
use guff_packages::{load, Config, LoadError, LoadMode, Package};

use crate::action::{analyze_with_settings, Graph};
use crate::cache::{load_from_cache, save_to_cache, CacheStats, IssueCache};
use crate::load_mode::load_mode_for_analyzers;
use crate::memory::trim_packages;

/// Options controlling runner behavior.
#[derive(Clone)]
pub struct RunnerOptions {
    /// Run analyzers sequentially (useful for tests and deterministic ordering).
    pub sequential: bool,
    /// Worker count for the action DAG (`-j` / `run.concurrency`).
    ///
    /// Ignored when [`Self::sequential`] is true or when set to `Some(1)`.
    /// `None` uses [`std::thread::available_parallelism`].
    pub concurrency: Option<usize>,
    /// After analysis, drop syntax and type artifacts from non-root packages.
    ///
    /// See [`crate::memory`] and deferral PL06 for full `decUse` semantics.
    pub release_memory: bool,
    /// Per-linter settings shared with every [`guff_analysis::Pass`].
    pub settings: Arc<SettingsBag>,
    /// Optional persistent issues cache (`GUFF_CACHE` / `GOLANGCI_LINT_CACHE`).
    pub cache: Option<Arc<IssueCache>>,
}

impl std::fmt::Debug for RunnerOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunnerOptions")
            .field("sequential", &self.sequential)
            .field("concurrency", &self.concurrency)
            .field("release_memory", &self.release_memory)
            .field("settings", &self.settings)
            .field("cache", &self.cache.as_ref().map(|c| c.dir()))
            .finish()
    }
}

impl Default for RunnerOptions {
    fn default() -> Self {
        Self {
            sequential: false,
            concurrency: None,
            release_memory: false,
            settings: Arc::new(SettingsBag::default()),
            cache: None,
        }
    }
}

/// Combined load + analyze error.
#[derive(Debug)]
pub enum RunnerError {
    Load(LoadError),
    Validate(ValidateError),
}

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(err) => write!(f, "{err}"),
            Self::Validate(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for RunnerError {}

/// Output from a runner invocation.
#[derive(Debug)]
pub struct RunResult {
    pub packages: Vec<Arc<Package>>,
    pub graph: Graph,
    /// Diagnostics restored from the persistent cache (not present in [`Self::graph`]).
    pub cached_diagnostics: Vec<(String, Diagnostic)>,
    /// Hit/miss summary when a cache was configured.
    pub cache_stats: CacheStats,
}

impl RunResult {
    pub fn diagnostics(&self) -> Vec<(String, Diagnostic)> {
        let mut out = self.graph.root_diagnostics();
        out.extend(self.cached_diagnostics.iter().cloned());
        out
    }
}

/// Runs analyzers on already-loaded packages.
///
/// When [`RunnerOptions::cache`] is set, unchanged packages are skipped and their
/// diagnostics are loaded from disk (golangci `loadIssuesFromCache`).
pub fn run_on_packages(
    analyzers: &[&'static Analyzer],
    packages: &[Arc<Package>],
    opts: &RunnerOptions,
) -> Result<RunResult, ValidateError> {
    let sequential = opts.sequential || opts.concurrency == Some(1);
    let concurrency = if sequential {
        None
    } else {
        opts.concurrency
    };

    let (cached_diagnostics, to_analyze, cache_stats) = if let Some(cache) = &opts.cache {
        load_from_cache(cache, packages)
    } else {
        (
            Vec::new(),
            packages.to_vec(),
            CacheStats::default(),
        )
    };

    let graph = if to_analyze.is_empty() {
        Graph::empty()
    } else {
        analyze_with_settings(
            analyzers,
            &to_analyze,
            sequential,
            concurrency,
            Arc::clone(&opts.settings),
            opts.cache.clone(),
        )?
    };

    if let Some(cache) = &opts.cache {
        save_to_cache(cache, &to_analyze, &graph.root_diagnostics());
    }

    let mut pkgs = packages.to_vec();
    if opts.release_memory {
        let root_ids: Vec<String> = packages.iter().map(|p| p.id.clone()).collect();
        trim_packages(&mut pkgs, &root_ids);
    }
    Ok(RunResult {
        packages: pkgs,
        graph,
        cached_diagnostics,
        cache_stats,
    })
}

/// Loads packages with the union of required load modes, then runs analyzers.
pub fn run(
    cfg: &Config,
    patterns: &[String],
    analyzers: &[&'static Analyzer],
    load_overrides: &HashMap<&'static str, LoadMode>,
    opts: &RunnerOptions,
) -> Result<RunResult, RunnerError> {
    let mode = load_mode_for_analyzers(analyzers, load_overrides).union(cfg.mode.normalize());
    let mut load_cfg = cfg.clone();
    load_cfg.mode = mode;
    let packages = load(&load_cfg, patterns).map_err(RunnerError::Load)?;
    run_on_packages(analyzers, &packages, opts).map_err(RunnerError::Validate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::{AnalysisResult, RunError};
    use guff_analysis::Pass;
    use guff_packages::{typecheck_package, TypecheckEnv};
    use guff_types::default_sizes;
    use guff::position::FileSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::OnceLock;

    static PKG_A_DONE: AtomicUsize = AtomicUsize::new(0);
    static PKG_B_DONE: AtomicUsize = AtomicUsize::new(0);

    fn parallel_run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
        let path = pass.pkg().pkg_path.as_str();
        if path.contains("pkg_a") {
            PKG_A_DONE.fetch_add(1, Ordering::SeqCst);
        } else if path.contains("pkg_b") {
            PKG_B_DONE.fetch_add(1, Ordering::SeqCst);
        }
        Ok(None)
    }

    fn parallel_analyzer() -> &'static Analyzer {
        static A: OnceLock<Analyzer> = OnceLock::new();
        A.get_or_init(|| Analyzer {
            name: "parallel",
            doc: "parallel test",
            url: "",
            run: parallel_run,
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        })
    }

    fn make_pkg(id: &str, file_name: &str) -> Arc<Package> {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../guff-packages/tests/testdata/typecheck/valid");
        let mut pkg = Package {
            id: id.into(),
            pkg_path: id.into(),
            dir: dir.clone(),
            compiled_go_files: vec![dir.join(file_name)],
            ..Package::default()
        };
        let fset = FileSet::new();
        typecheck_package(
            &mut pkg,
            &fset,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            default_sizes(),
            &TypecheckEnv::default(),
            LoadMode::LOAD_SYNTAX,
        );
        Arc::new(pkg)
    }

    #[test]
    fn two_packages_complete_independently() {
        PKG_A_DONE.store(0, Ordering::SeqCst);
        PKG_B_DONE.store(0, Ordering::SeqCst);

        let a = make_pkg("example.com/pkg_a", "main.go");
        let b = make_pkg("example.com/pkg_b", "main.go");
        let result = run_on_packages(
            &[parallel_analyzer()],
            &[a, b],
            &RunnerOptions::default(),
        )
        .expect("run");
        assert_eq!(result.graph.roots.len(), 2);
        assert_eq!(PKG_A_DONE.load(Ordering::SeqCst), 1);
        assert_eq!(PKG_B_DONE.load(Ordering::SeqCst), 1);
        for root in &result.graph.roots {
            assert!(root.error().is_none());
        }
    }

    #[test]
    fn release_memory_clears_non_root_syntax() {
        let pkg = make_pkg("example.com/root", "main.go");
        assert!(!pkg.syntax.is_empty());
        let mut packages = vec![Arc::clone(&pkg)];
        trim_packages(&mut packages, &["example.com/root".into()]);
        assert!(!packages[0].syntax.is_empty());

        let mut packages = vec![pkg];
        trim_packages(&mut packages, &["other".into()]);
        assert!(packages[0].syntax.is_empty());
    }
}
