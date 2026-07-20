//! Action graph construction and execution.
//!
//! Port of `golang.org/x/tools/go/analysis/checker` (`Action`, `Analyze`).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use guff::position::FileSet;
use guff_analysis::{
    decode_facts_into, encode_fact_store, ensure_builtin_fact_decoders, remap_facts, AnalysisResult,
    Analyzer, Diagnostic, EncodedFact, FactStore, PassInput, SettingsBag, ValidateError, validate,
};
use guff_packages::Package;
use guff_types::default_sizes;

use crate::cache::{HashMode, IssueCache};

/// One unit of analysis work: one analyzer applied to one package.
///
/// Equivalent to `checker.Action`.
pub struct Action {
    pub analyzer: &'static Analyzer,
    pub package: Arc<Package>,
    pub is_root: std::sync::atomic::AtomicBool,
    pub deps: Vec<Arc<Action>>,
    settings: Arc<SettingsBag>,
    /// Optional persistent issues/facts cache (R24).
    cache: Option<Arc<IssueCache>>,
    state: Mutex<ActionState>,
}

#[derive(Default)]
struct ActionState {
    result: Option<AnalysisResult>,
    error: Option<String>,
    diagnostics: Vec<Diagnostic>,
    facts: FactStore,
    /// Stable encoding for cross-arena inheritance and disk persistence.
    encoded_facts: Vec<EncodedFact>,
}

/// Debug-only per-analyzer wall-time + action-count accumulator, printed when
/// `GUFF_DEBUG_CACHE` is set. Times sum across worker threads so the total can
/// exceed wall-clock; the point is the *relative* cost and the action count
/// (which reveals fact-producing analyzers fanning out over dependencies).
static ANALYZER_TIMING: std::sync::LazyLock<Mutex<HashMap<&'static str, (u128, usize)>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn timing_enabled() -> bool {
    std::env::var_os("GUFF_DEBUG_CACHE").is_some()
}

fn record_analyzer_time(name: &'static str, nanos: u128) {
    let mut m = ANALYZER_TIMING.lock().unwrap_or_else(|e| e.into_inner());
    let entry = m.entry(name).or_insert((0, 0));
    entry.0 += nanos;
    entry.1 += 1;
}

/// Print and reset the per-analyzer timing table (top entries by total time).
pub(crate) fn report_analyzer_timing() {
    if !timing_enabled() {
        return;
    }
    let mut m = ANALYZER_TIMING.lock().unwrap_or_else(|e| e.into_inner());
    if m.is_empty() {
        return;
    }
    let mut rows: Vec<(&'static str, u128, usize)> =
        m.iter().map(|(k, (t, c))| (*k, *t, *c)).collect();
    rows.sort_by_key(|(_, t, _)| std::cmp::Reverse(*t));
    eprintln!("guff: per-analyzer analyze time (summed across workers, top 20):");
    for (name, nanos, count) in rows.iter().take(20) {
        eprintln!(
            "  {:>30} {:>9.2}s  {:>6} actions",
            name,
            *nanos as f64 / 1e9,
            count,
        );
    }
    m.clear();
}

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Action")
            .field("id", &self.string_id())
            .field("is_root", &self.is_root.load(Ordering::Relaxed))
            .field("deps", &self.deps.len())
            .finish()
    }
}

impl Action {
    pub fn string_id(&self) -> String {
        format!("{}@{}", self.analyzer.name, self.package.pkg_path)
    }

    pub fn result(&self) -> Option<AnalysisResult> {
        self.state.lock().unwrap().result.as_ref().map(clone_result)
    }

    fn result_arc(&self) -> Option<Arc<AnalysisResult>> {
        self.state
            .lock()
            .unwrap()
            .result
            .as_ref()
            .map(|r| Arc::new(clone_result(r)))
    }

    pub fn error(&self) -> Option<String> {
        self.state.lock().unwrap().error.clone()
    }

    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.state.lock().unwrap().diagnostics.clone()
    }

    fn execute(&self) {
        for dep in &self.deps {
            if let Some(err) = dep.error() {
                let mut state = self.state.lock().unwrap();
                state.error = Some(format!("failed prerequisites: {err}"));
                return;
            }
        }

        // Non-root fact producers: prefer loading persisted facts instead of
        // re-analyzing (golangci `loadCachedFacts` for non-initial packages).
        // This is the warm-path win when a dependency's issues hit the cache
        // and the package itself is not type-checked this run.
        if !self.is_root.load(Ordering::Relaxed)
            && !self.analyzer.fact_types.is_empty()
            && self.try_load_cached_facts()
        {
            return;
        }

        let mut result_of = HashMap::new();
        let mut facts = FactStore::default();

        for dep in &self.deps {
            let dep_state = dep.state.lock().unwrap();
            if Arc::ptr_eq(&dep.package, &self.package) {
                if let Some(result) = dep_state.result.as_ref() {
                    result_of.insert(dep.analyzer.name, Arc::new(clone_result(result)));
                }
            } else if std::ptr::eq(dep.analyzer, self.analyzer) {
                inherit_facts(self, dep, &dep_state, &mut facts);
            }
        }

        if self.package.ill_typed && !self.analyzer.run_despite_errors {
            let mut state = self.state.lock().unwrap();
            state.error = Some(format!(
                "analysis skipped: package {} is ill-typed",
                self.package.pkg_path
            ));
            return;
        }

        let fset = self
            .package
            .fset
            .clone()
            .unwrap_or_else(FileSet::new);
        let types_sizes = self.package.types_sizes.unwrap_or_else(default_sizes);
        let mut diagnostics = Vec::new();

        // Isolate linter panics: a bug in one analyzer on one package must not
        // unwind the rayon worker and abort the whole run (which surfaces as
        // "lint worker exited without a result"). Catch the panic here and turn
        // it into an ordinary action error, exactly as `Cargo.toml`'s
        // `panic = "unwind"` note promises. Dependents see the error and skip.
        let run_result = {
            let mut pass = PassInput {
                analyzer: self.analyzer,
                fset: &fset,
                files: &self.package.syntax,
                pkg: &self.package,
                types_info: self.package.types_info.as_ref(),
                types_sizes,
                diagnostics: &mut diagnostics,
                result_of,
                facts: &mut facts,
                settings: Arc::clone(&self.settings),
            }
            .build();

            let start = timing_enabled().then(std::time::Instant::now);
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (self.analyzer.run)(&mut pass)
            }));
            if let Some(start) = start {
                record_analyzer_time(self.analyzer.name, start.elapsed().as_nanos());
            }
            r
        };
        let run_result = match run_result {
            Ok(result) => result,
            Err(payload) => {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                state.error = Some(format!(
                    "analyzer {} panicked on {}: {}",
                    self.analyzer.name,
                    self.package.pkg_path,
                    panic_message(&payload),
                ));
                return;
            }
        };
        let encoded = encode_action_facts(&facts, &self.package);
        if let Some(cache) = &self.cache {
            if !self.analyzer.fact_types.is_empty() {
                let _ = cache.put_facts(
                    &self.package,
                    HashMode::NeedAllDeps,
                    self.analyzer.name,
                    &encoded,
                );
            }
        }
        let mut state = self.state.lock().unwrap();
        match run_result {
            Ok(Some(result)) => {
                state.result = Some(result);
                state.diagnostics = diagnostics;
                state.facts = facts;
                state.encoded_facts = encoded;
            }
            Ok(None) => {
                state.diagnostics = diagnostics;
                state.facts = facts;
                state.encoded_facts = encoded;
            }
            Err(err) => {
                state.error = Some(err);
            }
        }
    }

    /// Load persisted facts for this non-root action. Returns true on hit.
    fn try_load_cached_facts(&self) -> bool {
        let Some(cache) = &self.cache else {
            return false;
        };
        ensure_builtin_fact_decoders();
        let Ok(encoded) =
            cache.get_facts(&self.package, HashMode::NeedAllDeps, self.analyzer.name)
        else {
            return false;
        };
        let mut state = self.state.lock().unwrap();
        state.encoded_facts = encoded;
        // FactStore stays empty here — consumers inherit via encoded_facts remapping.
        true
    }
}

/// Inherit facts from a same-analyzer dependency action into `dst`.
///
/// Prefer objectpath remapping when both packages have type artifacts; otherwise
/// fall back to the dependency's encoded facts (from this run or the cache).
fn inherit_facts(
    self_act: &Action,
    dep: &Action,
    dep_state: &ActionState,
    dst: &mut FactStore,
) {
    ensure_builtin_fact_decoders();
    let Some(dst_arts) = self_act.package.type_artifacts.as_ref() else {
        return;
    };

    if let (Some(src_arts), true) = (
        dep.package.type_artifacts.as_ref(),
        !dep_state.facts.is_empty(),
    ) {
        remap_facts(
            &dep_state.facts,
            src_arts,
            &dep.package.pkg_path,
            dst_arts,
            dst,
        );
        return;
    }

    if !dep_state.encoded_facts.is_empty() {
        decode_facts_into(
            &dep_state.encoded_facts,
            dst_arts,
            &dep.package.pkg_path,
            dst,
        );
    }
}

/// Extract a human-readable message from a caught panic payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

fn encode_action_facts(store: &FactStore, pkg: &Package) -> Vec<EncodedFact> {
    let Some(arts) = pkg.type_artifacts.as_ref() else {
        return Vec::new();
    };
    encode_fact_store(store, arts, &pkg.pkg_path)
}

/// Result graph from a round of analysis.
#[derive(Debug, Default)]
pub struct Graph {
    pub roots: Vec<Arc<Action>>,
    all: Vec<Arc<Action>>,
}

impl Graph {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn all_actions(&self) -> &[Arc<Action>] {
        &self.all
    }

    pub fn root_diagnostics(&self) -> Vec<(String, Diagnostic)> {
        let mut out = Vec::new();
        for root in &self.roots {
            for diag in root.diagnostics() {
                out.push((root.string_id(), diag));
            }
        }
        out
    }
}

/// Builds and executes the action graph.
///
/// Port of `checker.Analyze`.
pub fn analyze(
    analyzers: &[&'static Analyzer],
    packages: &[Arc<Package>],
    sequential: bool,
) -> Result<Graph, ValidateError> {
    analyze_with_settings(
        analyzers,
        packages,
        sequential,
        None,
        Arc::new(SettingsBag::default()),
        None,
    )
}

/// Like [`analyze`], but forwards a shared [`SettingsBag`] into every [`Pass`].
pub fn analyze_with_settings(
    analyzers: &[&'static Analyzer],
    packages: &[Arc<Package>],
    sequential: bool,
    concurrency: Option<usize>,
    settings: Arc<SettingsBag>,
    cache: Option<Arc<IssueCache>>,
) -> Result<Graph, ValidateError> {
    validate(analyzers)?;
    ensure_builtin_fact_decoders();

    let mut actions: HashMap<(*const Analyzer, String), Arc<Action>> = HashMap::new();
    let mut all: Vec<Arc<Action>> = Vec::new();

    fn mk_action(
        analyzer: &'static Analyzer,
        package: Arc<Package>,
        settings: &Arc<SettingsBag>,
        cache: &Option<Arc<IssueCache>>,
        actions: &mut HashMap<(*const Analyzer, String), Arc<Action>>,
        all: &mut Vec<Arc<Action>>,
    ) -> Arc<Action> {
        let key = (analyzer as *const Analyzer, package.id.clone());
        if let Some(act) = actions.get(&key) {
            return Arc::clone(act);
        }

        let mut deps = Vec::new();
        for req in &analyzer.requires {
            deps.push(mk_action(
                req,
                Arc::clone(&package),
                settings,
                cache,
                actions,
                all,
            ));
            if !req.fact_types.is_empty() {
                let mut paths: Vec<String> = package.imports.keys().cloned().collect();
                paths.sort();
                for path in paths {
                    if let Some(dep_pkg) = package.imports.get(&path) {
                        deps.push(mk_action(
                            req,
                            Arc::clone(dep_pkg),
                            settings,
                            cache,
                            actions,
                            all,
                        ));
                    }
                }
            }
        }

        if !analyzer.fact_types.is_empty() {
            let mut paths: Vec<String> = package.imports.keys().cloned().collect();
            paths.sort();
            for path in paths {
                if let Some(dep_pkg) = package.imports.get(&path) {
                    deps.push(mk_action(
                        analyzer,
                        Arc::clone(dep_pkg),
                        settings,
                        cache,
                        actions,
                        all,
                    ));
                }
            }
        }

        let act = Arc::new(Action {
            analyzer,
            package,
            is_root: std::sync::atomic::AtomicBool::new(false),
            deps,
            settings: Arc::clone(settings),
            cache: cache.clone(),
            state: Mutex::new(ActionState::default()),
        });
        actions.insert(key, Arc::clone(&act));
        all.push(Arc::clone(&act));
        act
    }

    let mut roots = Vec::new();
    for &analyzer in analyzers {
        for pkg in packages {
            let act = mk_action(
                analyzer,
                Arc::clone(pkg),
                &settings,
                &cache,
                &mut actions,
                &mut all,
            );
            act.is_root.store(true, Ordering::Relaxed);
            roots.push(act);
        }
    }

    exec_all(&roots, sequential, concurrency);
    report_analyzer_timing();

    for act in &all {
        if !act.is_root.load(Ordering::Relaxed) {
            let mut state = act.state.lock().unwrap();
            state.result = None;
        }
    }

    Ok(Graph { roots, all })
}

fn topo_postorder(roots: &[Arc<Action>]) -> Vec<Arc<Action>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    fn visit(act: &Arc<Action>, seen: &mut HashSet<usize>, out: &mut Vec<Arc<Action>>) {
        let ptr = Arc::as_ptr(act) as usize;
        if seen.contains(&ptr) {
            return;
        }
        seen.insert(ptr);
        for dep in &act.deps {
            visit(dep, seen, out);
        }
        out.push(Arc::clone(act));
    }

    for root in roots {
        visit(root, &mut seen, &mut out);
    }
    out
}

pub(crate) fn exec_all(roots: &[Arc<Action>], sequential: bool, concurrency: Option<usize>) {
    let order = topo_postorder(roots);

    // Reverse-dependency counts, keyed by action pointer: how many not-yet-run
    // actions still consume each action's result. Intermediate results are only
    // read by an action's direct dependents (see `execute`), so once the last
    // dependent has run we drop the result immediately instead of holding every
    // result until the whole run finishes. This matters a lot for `buildir`,
    // whose result (an SSA `Program`) owns a *clone of the full type arena* —
    // on a large multi-package run (e.g. Prometheus) retaining all of them at
    // once dominated peak memory. Roots are never freed here (their diagnostics
    // are collected after the run); the post-run sweep still clears them.
    let remaining = reverse_dep_counts(&order);

    if sequential {
        for act in &order {
            act.execute();
            release_finished_deps(act, &remaining);
        }
        return;
    }

    let workers = concurrency.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    });
    if workers <= 1 {
        for act in &order {
            act.execute();
            release_finished_deps(act, &remaining);
        }
        return;
    }

    // Wavefront schedule: each wave is a maximal set of actions whose deps are
    // already done, then rayon's pool runs the wave in parallel. Matching the
    // sequential topo order's diagnostic root walk keeps output deterministic
    // after collection (roots still walk in construction order).
    // Rayon's default worker stack (~512 KiB on macOS) is too small for deep SSA
    // / type substitution on large modules; match the main thread's headroom.
    const WORKER_STACK: usize = 8 * 1024 * 1024;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .stack_size(WORKER_STACK)
        .build()
        .expect("rayon thread pool");
    let remaining = &remaining;
    pool.install(|| {
        for wave in dependency_waves(&order) {
            rayon::scope(|s| {
                for act in &wave {
                    let act = Arc::clone(act);
                    s.spawn(move |_| {
                        act.execute();
                        release_finished_deps(&act, remaining);
                    });
                }
            });
        }
    });
}

/// Counts, per action (keyed by `Arc` pointer), how many actions list it as a
/// dependency — i.e. how many consumers will read its result.
fn reverse_dep_counts(order: &[Arc<Action>]) -> HashMap<usize, AtomicUsize> {
    let mut counts: HashMap<usize, AtomicUsize> = HashMap::with_capacity(order.len());
    for act in order {
        counts
            .entry(Arc::as_ptr(act) as usize)
            .or_insert_with(|| AtomicUsize::new(0));
        for dep in &act.deps {
            counts
                .entry(Arc::as_ptr(dep) as usize)
                .or_insert_with(|| AtomicUsize::new(0))
                .fetch_add(1, Ordering::Relaxed);
        }
    }
    counts
}

/// After `act` has run, decrement each dependency's outstanding-consumer count;
/// when a non-root dependency reaches zero, drop its now-unneeded result.
fn release_finished_deps(act: &Arc<Action>, remaining: &HashMap<usize, AtomicUsize>) {
    for dep in &act.deps {
        let Some(count) = remaining.get(&(Arc::as_ptr(dep) as usize)) else {
            continue;
        };
        // `AcqRel` so the thread that observes the transition to zero has seen
        // every other consumer's decrement (and their reads of the result).
        if count.fetch_sub(1, Ordering::AcqRel) == 1 && !dep.is_root.load(Ordering::Relaxed) {
            dep.state.lock().unwrap().result = None;
        }
    }
}

/// Groups actions into waves where every action in a wave has all dependencies
/// in earlier waves (or no deps). Actions within a wave are independent.
fn dependency_waves(order: &[Arc<Action>]) -> Vec<Vec<Arc<Action>>> {
    let mut completed = HashSet::new();
    let mut remaining: HashSet<usize> = order
        .iter()
        .map(|a| Arc::as_ptr(a) as usize)
        .collect();
    let by_ptr: HashMap<usize, Arc<Action>> = order
        .iter()
        .map(|a| (Arc::as_ptr(a) as usize, Arc::clone(a)))
        .collect();

    let mut waves = Vec::new();
    while !remaining.is_empty() {
        let mut wave = Vec::new();
        for &ptr in &remaining {
            let act = &by_ptr[&ptr];
            if act
                .deps
                .iter()
                .all(|d| completed.contains(&(Arc::as_ptr(d) as usize)))
            {
                wave.push(Arc::clone(act));
            }
        }
        assert!(
            !wave.is_empty(),
            "action dependency cycle detected in exec_all"
        );
        // Stable order within a wave: match topo_postorder appearance.
        wave.sort_by_key(|a| {
            order
                .iter()
                .position(|o| Arc::ptr_eq(o, a))
                .unwrap_or(usize::MAX)
        });
        for act in &wave {
            let ptr = Arc::as_ptr(act) as usize;
            completed.insert(ptr);
            remaining.remove(&ptr);
        }
        waves.push(wave);
    }
    waves
}

fn clone_result(result: &AnalysisResult) -> AnalysisResult {
    if let Some(inspect) = result.downcast_ref::<guff_analysis::passes::inspect::InspectResult>() {
        return Box::new(inspect.clone());
    }
    if let Some(ir) = result.downcast_ref::<guff_analysis::passes::buildir::BuildIrResult>() {
        return Box::new(ir.clone());
    }
    if let Some(index) = result.downcast_ref::<guff_analysis::passes::typeindex::Index>() {
        return Box::new(index.clone());
    }
    if let Some(depr) = result.downcast_ref::<guff_analysis::DeprecatedResult>() {
        return Box::new(depr.clone());
    }
    if let Some(gen) = result.downcast_ref::<guff_analysis::GeneratedResult>() {
        return Box::new(gen.clone());
    }
    panic!("unsupported AnalysisResult clone; add a clone path for this result type");
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::{AnalysisResult, Pass, RunError};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::OnceLock;

    static ORDER: AtomicUsize = AtomicUsize::new(0);
    static LOG: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

    fn record_run(
        name: &'static str,
    ) -> impl Fn(&mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> + Copy {
        move |_pass: &mut Pass<'_>| {
            LOG.lock().unwrap().push(name);
            ORDER.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }
    }

    fn c_run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
        record_run("c")(pass)
    }

    fn b_run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
        record_run("b")(pass)
    }

    fn a_run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
        record_run("a")(pass)
    }

    fn analyzer(
        name: &'static str,
        requires: Vec<&'static Analyzer>,
        run: fn(&mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError>,
    ) -> &'static Analyzer {
        static A: OnceLock<Analyzer> = OnceLock::new();
        static B: OnceLock<Analyzer> = OnceLock::new();
        static C: OnceLock<Analyzer> = OnceLock::new();
        match name {
            "a" => A.get_or_init(|| Analyzer {
                name: "a",
                doc: "a",
                url: "",
                run,
                run_despite_errors: false,
                requires,
                fact_types: vec![],
            }),
            "b" => B.get_or_init(|| Analyzer {
                name: "b",
                doc: "b",
                url: "",
                run,
                run_despite_errors: false,
                requires,
                fact_types: vec![],
            }),
            "c" => C.get_or_init(|| Analyzer {
                name: "c",
                doc: "c",
                url: "",
                run,
                run_despite_errors: false,
                requires,
                fact_types: vec![],
            }),
            _ => panic!("unknown test analyzer {name}"),
        }
    }

    fn typechecked_pkg() -> Arc<Package> {
        use guff::position::FileSet;
        use guff_packages::{typecheck_package, LoadMode, TypecheckEnv};

        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../guff-packages/tests/testdata/typecheck/valid");
        let mut pkg = Package {
            id: "example.com/valid".into(),
            pkg_path: "example.com/valid".into(),
            dir: dir.clone(),
            compiled_go_files: vec![dir.join("main.go")],
            ..Package::default()
        };
        let fset = FileSet::new();
        typecheck_package(
            &mut pkg,
            &fset,
            &HashMap::new(),
            &HashMap::new(),
            default_sizes(),
            &TypecheckEnv::default(),
            LoadMode::LOAD_SYNTAX,
        );
        Arc::new(pkg)
    }

    #[test]
    fn requires_chain_runs_in_dependency_order() {
        let c = analyzer("c", vec![], c_run);
        let b = analyzer("b", vec![c], b_run);
        let a = analyzer("a", vec![b], a_run);

        *LOG.lock().unwrap() = Vec::new();
        ORDER.store(0, Ordering::SeqCst);

        let pkg = typechecked_pkg();
        let graph = analyze(&[a], std::slice::from_ref(&pkg), true).expect("analyze");
        assert_eq!(graph.roots.len(), 1);
        assert!(graph.roots[0].error().is_none());

        let log = LOG.lock().unwrap().clone();
        assert_eq!(log, vec!["c", "b", "a"]);
    }
}
