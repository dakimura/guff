//! Action graph construction and execution.
//!
//! Port of `golang.org/x/tools/go/analysis/checker` (`Action`, `Analyze`).

use crate::hash::{HashMap, HashSet};
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
    /// Shared behind an `Arc` so dependents read the producer's result by cloning
    /// the pointer, not deep-cloning the contents. `buildir`'s result owns a clone
    /// of the full type arena; the old per-dependent `clone_result` deep-copied it
    /// once per consuming analyzer *under the state lock*, which serialized the
    /// wave's workers and burned ~23s of extra user CPU on the Prometheus `./...`
    /// run (parallel was slower than sequential). Sharing the `Arc` removes both
    /// the copies and the contention, and keeps only one arena copy resident.
    result: Option<Arc<AnalysisResult>>,
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
    std::sync::LazyLock::new(|| Mutex::new(HashMap::default()));

/// Debug-only per-analyzer share of [`InspectResult::preorder`] time
/// (PERF_TASKS_V2 B-0). Keyed by analyzer name; value is
/// `(nanos, nodes scanned, nodes delivered)`.
///
/// Filled by diffing the calling thread's preorder counters across one action —
/// valid because an action runs to completion on the thread that started it.
///
/// `scanned` vs `delivered` is how B-1c progress is read: an analyzer still on
/// the unmasked API has the two equal, a migrated one delivers only the kinds
/// it asked for.
static PREORDER_BY_ANALYZER: std::sync::LazyLock<Mutex<HashMap<&'static str, (u64, u64, u64)>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::default()));

/// Debug-only per-package analyze accumulator, printed at `GUFF_DEBUG_CACHE=2`.
///
/// The per-analyzer table above sums CPU across workers, which answers "what is
/// expensive" but not "what is the run waiting on". Two sessions in a row cut
/// analyze CPU and moved wall by ~1% (gocritic memoization, the inspect kind
/// index), because the workers are only busy ~40% of the wall — so the number
/// that decides wall is the **critical path**, and its analyze half is the
/// slowest single package. This records both halves of that: summed CPU per
/// package, and the span from the package's first action starting to its last
/// finishing (which includes time the package spent blocked on a dependency).
static PACKAGE_TIMING: std::sync::LazyLock<Mutex<HashMap<String, PkgTiming>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::default()));

/// Debug-only `(package, analyzer) -> timing`, printed at `GUFF_DEBUG_CACHE=2`
/// for the package the analyze phase ends on.
///
/// The per-package table says *which* package the run waits for; this says what
/// that package spends its time in. Without it the tail package is a single
/// number and the only available move is to cut CPU everywhere — which is
/// exactly what moved wall by ~1% twice.
///
/// Each entry carries the same shape as [`PkgTiming`] — CPU *and* span — for the
/// same reason the per-package table does: the tail package's CPU exceeds its
/// span (its analyzers run in parallel), so the biggest CPU consumer is not
/// necessarily the one the package's span ends on. Cutting the former shrinks
/// CPU; only cutting the latter can shorten the phase.
static ANALYZER_BY_PACKAGE: std::sync::LazyLock<Mutex<HashMap<(String, &'static str), PkgTiming>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::default()));

/// Wall-clock origin for the span offsets in [`PkgTiming`]; first touched by
/// the first action to finish, i.e. the start of the analyze phase.
static ANALYZE_EPOCH: std::sync::LazyLock<std::time::Instant> =
    std::sync::LazyLock::new(std::time::Instant::now);

#[derive(Default, Clone, Copy)]
struct PkgTiming {
    /// Summed action time (across workers; can exceed the span).
    cpu_nanos: u128,
    actions: usize,
    /// Offsets from [`ANALYZE_EPOCH`], in nanoseconds.
    first_start: u128,
    last_end: u128,
}

impl PkgTiming {
    /// Fold one finished action in: CPU adds up, the span widens to cover it.
    fn merge(&mut self, nanos: u128, first_start: u128, last_end: u128) {
        self.cpu_nanos += nanos;
        self.actions += 1;
        if self.actions == 1 {
            self.first_start = first_start;
        } else {
            self.first_start = self.first_start.min(first_start);
        }
        self.last_end = self.last_end.max(last_end);
    }
}

fn timing_enabled() -> bool {
    std::env::var_os("GUFF_DEBUG_CACHE").is_some()
}

/// Whether the level-2 breakdown is on. Mirrors `guff-lint`'s `debug::detailed`;
/// the crates deliberately share no support crate, so keep the mapping in sync.
fn timing_detailed() -> bool {
    match std::env::var_os("GUFF_DEBUG_CACHE") {
        None => false,
        Some(v) => v
            .to_str()
            .and_then(|s| s.trim().parse::<u8>().ok())
            .unwrap_or(1)
            .max(1)
            >= 2,
    }
}

fn record_package_time(
    pkg_path: &str,
    analyzer: &'static str,
    start: std::time::Instant,
    nanos: u128,
) {
    let epoch = *ANALYZE_EPOCH;
    let first_start = start.saturating_duration_since(epoch).as_nanos();
    let last_end = first_start + nanos;
    if timing_detailed() {
        let mut m = ANALYZER_BY_PACKAGE.lock().unwrap_or_else(|e| e.into_inner());
        let entry = m.entry((pkg_path.to_string(), analyzer)).or_default();
        entry.merge(nanos, first_start, last_end);
    }
    let mut m = PACKAGE_TIMING.lock().unwrap_or_else(|e| e.into_inner());
    let entry = m.entry(pkg_path.to_string()).or_default();
    entry.merge(nanos, first_start, last_end);
}

/// Print and reset the per-package table: the slowest packages by CPU, and the
/// one whose span ends last (the tail the analyze phase cannot finish before).
fn report_package_timing() {
    if !timing_detailed() {
        return;
    }
    let mut m = PACKAGE_TIMING.lock().unwrap_or_else(|e| e.into_inner());
    if m.is_empty() {
        return;
    }
    let mut rows: Vec<(String, PkgTiming)> = m.drain().collect();
    drop(m);
    // Reset the companion table too, so a second run in the same process (the
    // watch mode) does not accumulate.
    let mut abp = ANALYZER_BY_PACKAGE.lock().unwrap_or_else(|e| e.into_inner());
    let by_pkg: HashMap<(String, &'static str), PkgTiming> = abp.drain().collect();
    drop(abp);

    let total_cpu: u128 = rows.iter().map(|(_, t)| t.cpu_nanos).sum();
    let span_end = rows.iter().map(|(_, t)| t.last_end).max().unwrap_or(0);
    rows.sort_by_key(|(_, t)| std::cmp::Reverse(t.cpu_nanos));

    eprintln!(
        "guff: per-package analyze time (top 20 of {} pkgs; {:.2}s total CPU, \
         {:.2}s from first action to last):",
        rows.len(),
        total_cpu as f64 / 1e9,
        span_end as f64 / 1e9,
    );
    for (path, t) in rows.iter().take(20) {
        eprintln!(
            "  {:>9.2}s CPU  {:>6} actions  [{:>6.2}s..{:>6.2}s]  {}",
            t.cpu_nanos as f64 / 1e9,
            t.actions,
            t.first_start as f64 / 1e9,
            t.last_end as f64 / 1e9,
            path,
        );
    }
    // The package the phase ends on — not necessarily the most expensive one,
    // which is the whole point of printing it separately.
    if let Some((path, t)) = rows.iter().max_by_key(|(_, t)| t.last_end) {
        eprintln!(
            "  tail: {} ends at {:.2}s ({:.2}s CPU over {:.2}s span)",
            path,
            t.last_end as f64 / 1e9,
            t.cpu_nanos as f64 / 1e9,
            (t.last_end - t.first_start) as f64 / 1e9,
        );
        report_analyzers_in_package(&by_pkg, path, *t);
    }
}

/// Print what the tail package spends its CPU on, and what its *span* ends on.
/// This is the number to act on: the phase cannot end before this package does,
/// and the package cannot finish faster than its own analyzer DAG.
///
/// Two orderings, because they answer different questions and the answers
/// differ. By CPU: what the package's work is made of — cut it and the machine
/// does less. By end offset: what the package is still waiting for when it
/// finishes — cut *that* and the phase gets shorter. A 20%-of-CPU analyzer that
/// finished at 40% of the span is free to delete and will not move wall at all.
fn report_analyzers_in_package(
    by_pkg: &HashMap<(String, &'static str), PkgTiming>,
    pkg_path: &str,
    pkg: PkgTiming,
) {
    let mut rows: Vec<(&'static str, PkgTiming)> = by_pkg
        .iter()
        .filter(|((p, _), _)| p == pkg_path)
        .map(|((_, a), t)| (*a, *t))
        .collect();
    if rows.is_empty() {
        return;
    }
    let secs = |n: u128| n as f64 / 1e9;
    let share = |n: u128| {
        if pkg.cpu_nanos > 0 {
            n as f64 / pkg.cpu_nanos as f64 * 100.0
        } else {
            0.0
        }
    };

    rows.sort_by_key(|(_, t)| std::cmp::Reverse(t.cpu_nanos));
    eprintln!(
        "  tail breakdown (top 15 of {} analyzers in that package, by CPU; \
         [start..end] are offsets into the analyze phase):",
        rows.len(),
    );
    for (name, t) in rows.iter().take(15) {
        eprintln!(
            "    {:>30} {:>8.3}s  {:>5.1}%  [{:>6.2}s..{:>6.2}s]",
            name,
            secs(t.cpu_nanos),
            share(t.cpu_nanos),
            secs(t.first_start),
            secs(t.last_end),
        );
    }

    // The critical tail *within* the tail package: the analyzers still running
    // when it finishes. Sorted by end offset so the last line is the one the
    // package's span actually ends on.
    //
    // `waited` — the gap between the package's first action and this analyzer's
    // first — separates the two reasons an analyzer can end last. Large CPU and
    // small `waited` means it is slow: make it cheaper. Small CPU and large
    // `waited` means it barely ran and simply started late, so making it cheaper
    // cannot help.
    //
    // `waited` does not say *why* it started late: its dependencies may still
    // have been running, or they may have finished long before and the workers
    // were busy elsewhere. Check the analyzer's `requires` and read each
    // dependency's own `[start..end]` above to tell those apart. On Prometheus's
    // tsdb, SA4006 / SA4010 / SA9005 require only `buildir` (and `inspect`),
    // `buildir` ended at 0.34s, and all three still start past 0.81s — for those
    // three it is ordering, not the DAG.
    rows.sort_by_key(|(_, t)| t.last_end);
    eprintln!("  tail critical path (last 5 analyzers to finish in that package):");
    for (name, t) in rows.iter().rev().take(5).rev() {
        eprintln!(
            "    {:>30} [{:>6.2}s..{:>6.2}s]  {:>8.3}s CPU ({:>5.1}%)  waited {:>6.2}s",
            name,
            secs(t.first_start),
            secs(t.last_end),
            secs(t.cpu_nanos),
            share(t.cpu_nanos),
            secs(t.first_start.saturating_sub(pkg.first_start)),
        );
    }
}

fn record_analyzer_time(name: &'static str, nanos: u128) {
    let mut m = ANALYZER_TIMING.lock().unwrap_or_else(|e| e.into_inner());
    let entry = m.entry(name).or_insert((0, 0));
    entry.0 += nanos;
    entry.1 += 1;
}

fn record_preorder_share(name: &'static str, nanos: u64, nodes: u64, hits: u64) {
    if nanos == 0 && nodes == 0 {
        return;
    }
    let mut m = PREORDER_BY_ANALYZER.lock().unwrap_or_else(|e| e.into_inner());
    let entry = m.entry(name).or_insert((0, 0, 0));
    entry.0 += nanos;
    entry.1 += nodes;
    entry.2 += hits;
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
    let analyze_total: u128 = rows.iter().map(|(_, t, _)| *t).sum();
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
    drop(m);
    report_preorder_timing(analyze_total);
    report_package_timing();
}

/// Print the B-0 measurement: how much of the analyze phase is spent inside
/// `InspectResult::preorder` re-walking ASTs.
///
/// `analyze_total` is the summed per-analyzer CPU from the table above, so the
/// share is apples-to-apples (both are summed across workers, not wall).
fn report_preorder_timing(analyze_total: u128) {
    if !guff_analysis::preorder_stats_enabled() {
        return;
    }
    let (calls, nodes, nanos, hits) = guff_analysis::preorder_totals();
    if calls == 0 {
        return;
    }
    let share = if analyze_total > 0 {
        nanos as f64 / analyze_total as f64 * 100.0
    } else {
        0.0
    };
    eprintln!(
        "guff: inspect preorder: {calls} calls, {nodes} nodes scanned, \
         {hits} delivered ({:.0}% filtered by mask{}), \
         {:.2}s total CPU ({share:.1}% of analyze CPU)",
        if nodes > 0 {
            (1.0 - hits as f64 / nodes as f64) * 100.0
        } else {
            0.0
        },
        if guff_analysis::masks_enabled() {
            ""
        } else {
            "; DISABLED via GUFF_INSPECT_MASKS=0"
        },
        nanos as f64 / 1e9,
    );
    let mut by = PREORDER_BY_ANALYZER.lock().unwrap_or_else(|e| e.into_inner());
    let mut rows: Vec<(&'static str, u64, u64, u64)> =
        by.iter().map(|(k, (t, n, h))| (*k, *t, *n, *h)).collect();
    rows.sort_by_key(|(_, t, _, _)| std::cmp::Reverse(*t));
    eprintln!("guff: preorder CPU by analyzer (top 20):");
    for (name, ns, nd, nh) in rows.iter().take(20) {
        eprintln!(
            "  {:>30} {:>9.2}s  {:>12} scanned  {:>12} delivered",
            name,
            *ns as f64 / 1e9,
            nd,
            nh,
        );
    }
    by.clear();
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
        self.state
            .lock()
            .unwrap()
            .result
            .as_ref()
            .map(|r| clone_result(r))
    }

    pub fn error(&self) -> Option<String> {
        self.state.lock().unwrap().error.clone()
    }

    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.state.lock().unwrap().diagnostics.clone()
    }

    fn execute(&self) {
        // Hard prerequisites are same-package `requires` (e.g. buildir). Same-analyzer
        // actions on imported packages exist only to produce facts — if those fail
        // (no types/syntax on an export-only dep), continue without their facts
        // rather than aborting the whole action. Otherwise a single untyped import
        // (stdlib / module cache) would silence fact producers like contextcheck
        // on well-typed roots (helm pkg/kube ↔ deployment/util).
        for dep in &self.deps {
            if !Arc::ptr_eq(&dep.package, &self.package) {
                continue;
            }
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

        let mut result_of: std::collections::HashMap<&'static str, Arc<_>> =
            std::collections::HashMap::new();
        let mut facts = FactStore::default();

        for dep in &self.deps {
            let dep_state = dep.state.lock().unwrap();
            if Arc::ptr_eq(&dep.package, &self.package) {
                if let Some(result) = dep_state.result.as_ref() {
                    // Share the producer's `Arc` — cheap pointer clone, no arena copy.
                    result_of.insert(dep.analyzer.name, Arc::clone(result));
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
            // DEBUG: surface typecheck errors once per package when skipping.
            if std::env::var_os("GUFF_DEBUG_ILL_TYPED").is_some() {
                static ONCE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
                    std::sync::OnceLock::new();
                let seen = ONCE.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
                let mut g = seen.lock().unwrap();
                if g.insert(self.package.pkg_path.clone()) {
                    eprintln!(
                        "guff: ill_typed {} ({} errors):",
                        self.package.pkg_path,
                        self.package.errors.len()
                    );
                    // Resolve each error's position through the package's own
                    // FileSet — the raw offsets are into the shared position
                    // space and are useless on their own.
                    let fset = self.package.fset.clone();
                    for e in self.package.errors.iter().take(20) {
                        let at = match (&fset, e.pos.parse::<i64>()) {
                            (Some(fs), Ok(off)) => {
                                let p = fs.position(guff::position::Pos(off));
                                if p.filename.is_empty() {
                                    e.pos.clone()
                                } else {
                                    format!("{}:{}:{}", p.filename, p.line, p.column)
                                }
                            }
                            _ => e.pos.clone(),
                        };
                        eprintln!("  {at}: {} ({:?})", e.msg, e.kind);
                    }
                }
            }
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
                pkg_arc: Some(Arc::clone(&self.package)),
                types_info: self.package.types_info.as_deref(),
                types_sizes,
                diagnostics: &mut diagnostics,
                result_of,
                facts: &mut facts,
                settings: Arc::clone(&self.settings),
            }
            .build();

            let start = timing_enabled().then(std::time::Instant::now);
            let pre_before = guff_analysis::preorder_thread_totals();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (self.analyzer.run)(&mut pass)
            }));
            if let Some(start) = start {
                let nanos = start.elapsed().as_nanos();
                record_analyzer_time(self.analyzer.name, nanos);
                record_package_time(&self.package.pkg_path, self.analyzer.name, start, nanos);
                let pre_after = guff_analysis::preorder_thread_totals();
                record_preorder_share(
                    self.analyzer.name,
                    pre_after.2.saturating_sub(pre_before.2),
                    pre_after.1.saturating_sub(pre_before.1),
                    pre_after.3.saturating_sub(pre_before.3),
                );
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
                state.result = Some(Arc::new(result));
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

    let mut actions: HashMap<(*const Analyzer, String), Arc<Action>> = HashMap::default();
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
            if analyzer_schedules_import_facts(req, settings) {
                let mut paths: Vec<String> = package.imports.keys().cloned().collect();
                paths.sort();
                for path in paths {
                    if let Some(dep_pkg) = package.imports.get(&path) {
                        // Fact producers need a typechecked package (SSA). Skip
                        // export-only / not-yet-typed import stubs.
                        if dep_pkg.type_artifacts.is_none() {
                            continue;
                        }
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

        if analyzer_schedules_import_facts(analyzer, settings) {
            let mut paths: Vec<String> = package.imports.keys().cloned().collect();
            paths.sort();
            for path in paths {
                if let Some(dep_pkg) = package.imports.get(&path) {
                    if dep_pkg.type_artifacts.is_none() {
                        continue;
                    }
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
    // Import-gate skip/keep counters (debug only). Keys are analyzer names.
    let mut gate_skip: HashMap<&'static str, usize> = HashMap::default();
    let mut gate_keep: HashMap<&'static str, usize> = HashMap::default();
    // One memo for the whole pass: the transitive-import queries below are
    // repeated for every (analyzer, package) pair.
    let mut dep_memo = DepMemo::default();
    for &analyzer in analyzers {
        for pkg in packages {
            if !analyzer_applies_to_package(analyzer, pkg, &mut dep_memo) {
                *gate_skip.entry(analyzer.name).or_default() += 1;
                continue;
            }
            if matches!(
                analyzer.name,
                "testifylint"
                    | "exptostd"
                    | "sloglint"
                    | "loggercheck"
                    | "fatcontext"
                    | "lostcancel"
                    | "SA1012"
                    | "SA1029"
                    | "SA1032"
                    | "SA1017"
                    | "SA1027"
                    | "SA1020"
                    | "SA1002"
                    | "SA1004"
                    | "SA1015"
                    | "SA1000"
                    | "atomic"
                    | "sigchanyzer"
                    | "httpresponse"
                    | "defers"
                    | "timeformat"
                    | "cgocall"
                    | "slog"
                    | "errorsas"
                    | "unmarshal"
            ) {
                *gate_keep.entry(analyzer.name).or_default() += 1;
            }
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
    if timing_enabled() {
        report_import_gate("testifylint", &gate_keep, &gate_skip, "no testify import");
        report_import_gate(
            "exptostd",
            &gate_keep,
            &gate_skip,
            "no x/exp/{maps,slices,constraints} import",
        );
        report_import_gate("sloglint", &gate_keep, &gate_skip, "no log/slog import");
        report_import_gate(
            "loggercheck",
            &gate_keep,
            &gate_skip,
            "no known logger import",
        );
        report_import_gate("fatcontext", &gate_keep, &gate_skip, "no context import");
        report_import_gate("lostcancel", &gate_keep, &gate_skip, "no context import");
        report_import_gate("SA1012", &gate_keep, &gate_skip, "no context import");
        report_import_gate("SA1029", &gate_keep, &gate_skip, "no context import");
        report_import_gate("atomic", &gate_keep, &gate_skip, "no sync/atomic import");
        report_import_gate("SA1027", &gate_keep, &gate_skip, "no sync/atomic import");
        report_import_gate("sigchanyzer", &gate_keep, &gate_skip, "no os/signal import");
        report_import_gate("SA1017", &gate_keep, &gate_skip, "no os/signal import");
        report_import_gate("httpresponse", &gate_keep, &gate_skip, "no net/http import");
        report_import_gate("SA1020", &gate_keep, &gate_skip, "no net/http import");
        report_import_gate("defers", &gate_keep, &gate_skip, "no time import");
        report_import_gate("timeformat", &gate_keep, &gate_skip, "no time import");
        report_import_gate("SA1002", &gate_keep, &gate_skip, "no time import");
        report_import_gate("SA1004", &gate_keep, &gate_skip, "no time import");
        report_import_gate("SA1015", &gate_keep, &gate_skip, "no time import");
        report_import_gate("cgocall", &gate_keep, &gate_skip, "no cgo import");
        report_import_gate("slog", &gate_keep, &gate_skip, "no log/slog import");
        report_import_gate("errorsas", &gate_keep, &gate_skip, "no errors import");
        report_import_gate("SA1032", &gate_keep, &gate_skip, "no errors import");
        report_import_gate("unmarshal", &gate_keep, &gate_skip, "no encoding/* import");
        report_import_gate("SA1000", &gate_keep, &gate_skip, "no regexp import");
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

/// Whether a root analyzer should be scheduled for `package`.
///
/// Whether this analyzer should run on imported packages to produce facts for
/// importers. Default: any analyzer with non-empty `fact_types`.
///
/// `modernize` advertises `NewLikeFact` for the `newexpr` check only. When that
/// check is disabled (settings flag from guff-lint), skip the import fan-out —
/// otherwise modernize runs on every transitive import (~1000 extra actions on
/// prometheus) with no consumer for the facts.
fn analyzer_schedules_import_facts(analyzer: &Analyzer, settings: &SettingsBag) -> bool {
    if analyzer.fact_types.is_empty() {
        return false;
    }
    if analyzer.name == "modernize" {
        return settings
            .get::<bool>("modernize_schedule_facts")
            .copied()
            .unwrap_or(true);
    }
    true
}

/// Import-gated skips must preserve findings: only omit analyzers that cannot
/// produce diagnostics without a given import. `buildir` is *not* gated here —
/// many staticcheck SA checks require it regardless of testify.
fn analyzer_applies_to_package(analyzer: &Analyzer, package: &Package, memo: &mut DepMemo) -> bool {
    match analyzer.name {
        "testifylint" => package_imports_prefix(package, "github.com/stretchr/testify"),
        // exptostd only rewrites `golang.org/x/exp/{maps,slices,constraints}`.
        "exptostd" => {
            package_imports_prefix(package, "golang.org/x/exp/maps")
                || package_imports_prefix(package, "golang.org/x/exp/slices")
                || package_imports_prefix(package, "golang.org/x/exp/constraints")
        }
        // sloglint / govet slog inspect `log/slog` APIs, but a `*slog.Logger`
        // handle is routinely built by a helper package and passed in, so the
        // call site's own file need not import `log/slog`. Gate on the
        // transitive closure — an http interceptor that calls
        // `logging.FromContext(ctx).WarnContext(...)` has no direct import.
        "sloglint" | "slog" => package_depends_on_prefix(package, "log/slog", memo),
        // loggercheck only fires on kitlog / klog / logr / slog / zap call
        // sites; same handle-passed-in caveat as sloglint.
        "loggercheck" => package_has_loggercheck_import(package, memo),
        "zerologlint" => package_depends_on_prefix(package, "github.com/rs/zerolog", memo),
        // fatcontext / lostcancel / SA1012 / SA1029 only look at `context` APIs.
        "fatcontext" | "lostcancel" | "SA1012" | "SA1029" => {
            package_imports_prefix(package, "context")
        }
        // errors.As / errors.Is only.
        "errorsas" | "SA1032" => package_imports_prefix(package, "errors"),
        // Analyzers that only fire on call sites in these packages.
        "atomic" | "SA1027" => package_imports_prefix(package, "sync/atomic"),
        "sigchanyzer" | "SA1017" => package_imports_prefix(package, "os/signal"),
        "httpresponse" | "SA1020" => package_imports_prefix(package, "net/http"),
        "defers" | "timeformat" | "SA1002" | "SA1004" | "SA1015" => {
            package_imports_prefix(package, "time")
        }
        "SA1000" | "SA6000" => package_imports_prefix(package, "regexp"),
        "SA1003" => package_imports_prefix(package, "encoding/binary"),
        "SA1007" => package_imports_prefix(package, "net/url"),
        "SA1014" | "SA1026" | "SA9005" => {
            package_imports_prefix(package, "encoding/json")
                || package_imports_prefix(package, "encoding/xml")
        }
        "SA1016" => package_imports_prefix(package, "os/signal"),
        "SA1028" => package_imports_prefix(package, "sort"),
        "SA1030" => package_imports_prefix(package, "strconv"),
        "SA1031" => {
            package_imports_prefix(package, "encoding/hex")
                || package_imports_prefix(package, "encoding/base64")
                || package_imports_prefix(package, "encoding/base32")
        }
        "durationcheck" => package_imports_prefix(package, "time"),
        "ginkgolinter" => {
            package_imports_prefix(package, "github.com/onsi/ginkgo")
                || package_imports_prefix(package, "github.com/onsi/gomega")
        }
        "clickhouselint" => {
            package_imports_prefix(package, "github.com/ClickHouse/clickhouse-go")
        }
        "unmarshal" => {
            package_imports_prefix(package, "encoding/json")
                || package_imports_prefix(package, "encoding/xml")
                || package_imports_prefix(package, "encoding/asn1")
                || package_imports_prefix(package, "encoding/gob")
        }
        "cgocall" => {
            package_imports_prefix(package, "runtime/cgo")
                || package_imports_prefix(package, "C")
        }
        _ => true,
    }
}

fn report_import_gate(
    name: &str,
    keep: &HashMap<&'static str, usize>,
    skip: &HashMap<&'static str, usize>,
    reason: &str,
) {
    let k = keep.get(name).copied().unwrap_or(0);
    let s = skip.get(name).copied().unwrap_or(0);
    if k == 0 && s == 0 {
        return;
    }
    eprintln!("guff:   {name} schedule keep={k} skip={s} ({reason})");
}

fn package_has_loggercheck_import(package: &Package, memo: &mut DepMemo) -> bool {
    [
        "github.com/go-kit/log",
        "github.com/go-kit/kit/log",
        "k8s.io/klog",
        "github.com/go-logr/logr",
        "log/slog",
        "go.uber.org/zap",
    ]
    .iter()
    .any(|prefix| package_depends_on_prefix(package, prefix, memo))
}

/// True when `prefix` is in the package's **transitive** dependency closure.
///
/// Needed for linters whose subject is a *value* (a logger handle) that another
/// package can construct and hand over: gating those on a direct import
/// silently drops findings, which [`analyzer_applies_to_package`]'s contract
/// forbids.
///
/// `go list`'s precomputed `deps` is only populated under `NEED_DEPS`, so fall
/// back to walking the `imports` graph. Nodes are shared `Arc<Package>`s and the
/// visited set is keyed by id, so each walk is linear in the closure size.
fn import_path_matches(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Memo for [`package_depends_on_prefix`], shared across one scheduling pass.
///
/// Keyed by node address rather than `Package::id` because synthetic packages
/// (tests, stubs) share an empty id. Entries are only valid while the package
/// graph they were computed from is alive, which is the whole scheduling pass.
#[derive(Default)]
pub(crate) struct DepMemo {
    reaches: HashMap<(usize, &'static str), bool>,
}

/// True when `prefix` is in the package's **transitive** dependency closure.
///
/// Needed for linters whose subject is a *value* (a logger handle) that another
/// package can construct and hand over: gating those on a direct import
/// silently drops findings, which [`analyzer_applies_to_package`]'s contract
/// forbids.
///
/// `go list`'s precomputed `deps` is only populated under `NEED_DEPS`, so this
/// falls back to walking the `imports` graph. Every visited node is memoized,
/// making a full scheduling pass O(V + E) instead of O(V x closure) — without
/// the memo this cost ~10% wall on the prometheus `full` regress profile.
fn package_depends_on_prefix(package: &Package, prefix: &'static str, memo: &mut DepMemo) -> bool {
    fn reaches(
        pkg: &Package,
        prefix: &'static str,
        memo: &mut DepMemo,
        on_stack: &mut HashSet<usize>,
    ) -> bool {
        let key = (std::ptr::from_ref(pkg) as usize, prefix);
        if let Some(&hit) = memo.reaches.get(&key) {
            return hit;
        }
        // Go import graphs are acyclic; guard anyway so a malformed graph
        // cannot spin forever.
        if !on_stack.insert(key.0) {
            return false;
        }
        let mut found = pkg.deps.iter().any(|p| import_path_matches(p, prefix));
        if !found {
            for (path, dep) in &pkg.imports {
                if import_path_matches(path, prefix) || reaches(dep, prefix, memo, on_stack) {
                    found = true;
                    break;
                }
            }
        }
        on_stack.remove(&key.0);
        memo.reaches.insert(key, found);
        found
    }

    if package_imports_prefix(package, prefix) {
        return true;
    }
    reaches(package, prefix, memo, &mut HashSet::default())
}

/// True when `package.imports` contains `prefix` or a subpath of it.
fn package_imports_prefix(package: &Package, prefix: &str) -> bool {
    package.imports.keys().any(|path| {
        path == prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn topo_postorder(roots: &[Arc<Action>]) -> Vec<Arc<Action>> {
    let mut seen = HashSet::default();
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
        // Analyze SSA overlays are small vs the shared seed base; allow full
        // ncpu here. Seed/target typecheck stay on the capped global pool.
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

    // Dependency-driven schedule: an action is spawned the moment its last
    // dependency finishes, by the worker that finished it. No global barrier.
    //
    // The wavefront schedule this replaced ran every package's `inspect`, then
    // every package's `gocritic`, and so on: with 118 root packages the phase
    // held 118 live `InspectResult` / `BuildIrResult` / `Index` values at once,
    // because a package's producer result cannot be dropped until its last
    // consumer runs — and under a barrier that consumer is in a later wave, i.e.
    // after every other package has caught up. The per-package table showed it
    // directly: all 118 packages spanned [0.00s..0.79s] of a 0.81s phase.
    // Releasing on the finishing worker instead lets one package run its whole
    // analyzer chain while the results are still hot, so only the packages
    // actually in flight (≈ worker count) are resident. Peak RSS is the point —
    // prometheus `./...` went 3.13 → 2.53 GiB — and the analyze phase also got
    // faster (0.84s → 0.66s wall, 7.2s → 6.0s CPU) because a package's results
    // stay in cache across its own analyzers instead of being revisited a wave
    // later.
    //
    // Rayon's default worker stack (~512 KiB on macOS) is too small for deep SSA
    // / type substitution on large modules; match the main thread's headroom.
    const WORKER_STACK: usize = 8 * 1024 * 1024;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .stack_size(WORKER_STACK)
        .build()
        .expect("rayon thread pool");
    let sched = Sched::new(&order, remaining);
    pool.install(|| {
        rayon::scope(|s| {
            for &i in &sched.initial_ready {
                let sched = &sched;
                s.spawn(move |s| sched.run(s, i as usize));
            }
        })
    });
    // A dependency counter can only fail to reach zero if the graph has a cycle,
    // which `validate` rules out for `requires` and the import graph rules out
    // for facts. If one ever slips through, run the leftovers rather than let an
    // analyzer silently produce no diagnostics — a missing finding is the one
    // outcome this scheduler must never have.
    for (i, act) in order.iter().enumerate() {
        if sched.indeg[i].load(Ordering::Acquire) != 0 {
            act.execute();
            release_finished_deps(act, &sched.remaining);
        }
    }
}

/// Ready-queue state for the dependency-driven analyze schedule.
///
/// `indeg[i]` counts `order[i]`'s not-yet-finished dependencies; the worker that
/// takes it to zero owns spawning it. Edges are counted with multiplicity, so a
/// duplicated dependency decrements twice and still reaches zero exactly once.
struct Sched<'a> {
    order: &'a [Arc<Action>],
    /// `dependents[i]` = indices whose `deps` contain `order[i]`.
    dependents: Vec<Vec<u32>>,
    indeg: Vec<AtomicUsize>,
    /// Outstanding-consumer counts for [`release_finished_deps`].
    remaining: HashMap<usize, AtomicUsize>,
    /// Actions with no dependencies, grouped so that consecutive entries belong
    /// to the same package. Rayon hands stealing workers consecutive tasks, so
    /// grouping keeps them on one package instead of fanning across all 118 —
    /// the same reason the barrier had to go. Ordering the packages themselves
    /// (longest-processing-time first, by source bytes) was tried and measured
    /// flat on both analyze wall and RSS, so the order stays first-seen.
    initial_ready: Vec<u32>,
}

impl<'a> Sched<'a> {
    fn new(order: &'a [Arc<Action>], remaining: HashMap<usize, AtomicUsize>) -> Self {
        let mut index: HashMap<usize, u32> =
            HashMap::with_capacity_and_hasher(order.len(), Default::default());
        for (i, act) in order.iter().enumerate() {
            index.insert(Arc::as_ptr(act) as usize, i as u32);
        }
        let mut dependents: Vec<Vec<u32>> = vec![Vec::new(); order.len()];
        let mut indeg: Vec<AtomicUsize> = Vec::with_capacity(order.len());
        for (i, act) in order.iter().enumerate() {
            indeg.push(AtomicUsize::new(act.deps.len()));
            for dep in &act.deps {
                // `order` is a topological post-order over the same graph, so
                // every dependency is already indexed.
                if let Some(&d) = index.get(&(Arc::as_ptr(dep) as usize)) {
                    dependents[d as usize].push(i as u32);
                }
            }
        }
        let mut initial_ready: Vec<u32> = (0..order.len() as u32)
            .filter(|&i| order[i as usize].deps.is_empty())
            .collect();
        // Group by package (stable within a package, packages in first-seen
        // order) so the initial fan-out does not scatter workers over every
        // package at once. Measured on prometheus `./...`: grouped analyze
        // 0.66-0.68s vs ungrouped 0.68-0.72s; peak RSS is the same either way
        // (that half is the barrier removal, not the order).
        let mut pkg_rank: HashMap<usize, u32> = HashMap::default();
        for &i in &initial_ready {
            let key = Arc::as_ptr(&order[i as usize].package) as usize;
            let next = pkg_rank.len() as u32;
            pkg_rank.entry(key).or_insert(next);
        }
        initial_ready.sort_by_key(|&i| {
            let key = Arc::as_ptr(&order[i as usize].package) as usize;
            (pkg_rank[&key], i)
        });
        Self {
            order,
            dependents,
            indeg,
            remaining,
            initial_ready,
        }
    }

    fn run<'scope>(&'scope self, s: &rayon::Scope<'scope>, i: usize) {
        let act = &self.order[i];
        act.execute();
        release_finished_deps(act, &self.remaining);
        for &d in &self.dependents[i] {
            let d = d as usize;
            // `AcqRel` for the same reason as `release_finished_deps`: the
            // worker that observes zero must see every other dependency's
            // writes to the action state it is about to read.
            if self.indeg[d].fetch_sub(1, Ordering::AcqRel) == 1 {
                s.spawn(move |s| self.run(s, d));
            }
        }
    }
}

/// Counts, per action (keyed by `Arc` pointer), how many actions list it as a
/// dependency — i.e. how many consumers will read its result.
fn reverse_dep_counts(order: &[Arc<Action>]) -> HashMap<usize, AtomicUsize> {
    let mut counts: HashMap<usize, AtomicUsize> = HashMap::with_capacity_and_hasher(order.len(), Default::default());
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
            &crate::hash::HashMap::default(),
            &crate::hash::HashMap::default(),
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

    #[test]
    fn package_imports_prefix_matches_module_and_subpath() {
        let mut pkg = Package {
            id: "example.com/p".into(),
            pkg_path: "example.com/p".into(),
            ..Package::default()
        };
        assert!(!package_imports_prefix(&pkg, "github.com/stretchr/testify"));
        pkg.imports.insert(
            "github.com/stretchr/testify/require".into(),
            Arc::new(Package::default()),
        );
        assert!(package_imports_prefix(&pkg, "github.com/stretchr/testify"));
        pkg.imports.clear();
        pkg.imports.insert(
            "github.com/stretchr/testifyextra".into(),
            Arc::new(Package::default()),
        );
        assert!(!package_imports_prefix(&pkg, "github.com/stretchr/testify"));
    }

    #[test]
    fn testifylint_skipped_without_testify_import() {
        let analyzer = Analyzer {
            name: "testifylint",
            doc: "",
            url: "",
            run: |_p| Ok(None),
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        };
        let pkg = Package::default();
        assert!(!analyzer_applies_to_package(&analyzer, &pkg, &mut DepMemo::default()));
        let mut with_testify = Package::default();
        with_testify.imports.insert(
            "github.com/stretchr/testify/assert".into(),
            Arc::new(Package::default()),
        );
        assert!(analyzer_applies_to_package(&analyzer, &with_testify, &mut DepMemo::default()));
    }

    #[test]
    fn exptostd_skipped_without_x_exp_import() {
        let analyzer = Analyzer {
            name: "exptostd",
            doc: "",
            url: "",
            run: |_p| Ok(None),
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        };
        let pkg = Package::default();
        assert!(!analyzer_applies_to_package(&analyzer, &pkg, &mut DepMemo::default()));
        // Unrelated x/exp subpackage must not keep the analyzer.
        let mut with_other = Package::default();
        with_other.imports.insert(
            "golang.org/x/exp/slog".into(),
            Arc::new(Package::default()),
        );
        assert!(!analyzer_applies_to_package(&analyzer, &with_other, &mut DepMemo::default()));
        let mut with_maps = Package::default();
        with_maps.imports.insert(
            "golang.org/x/exp/maps".into(),
            Arc::new(Package::default()),
        );
        assert!(analyzer_applies_to_package(&analyzer, &with_maps, &mut DepMemo::default()));
    }

    #[test]
    fn sloglint_skipped_without_slog_import() {
        let analyzer = Analyzer {
            name: "sloglint",
            doc: "",
            url: "",
            run: |_p| Ok(None),
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        };
        let pkg = Package::default();
        assert!(!analyzer_applies_to_package(&analyzer, &pkg, &mut DepMemo::default()));
        let mut with_slog = Package::default();
        with_slog
            .imports
            .insert("log/slog".into(), Arc::new(Package::default()));
        assert!(analyzer_applies_to_package(&analyzer, &with_slog, &mut DepMemo::default()));
    }

    #[test]
    fn loggercheck_skipped_without_logger_import() {
        let analyzer = Analyzer {
            name: "loggercheck",
            doc: "",
            url: "",
            run: |_p| Ok(None),
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        };
        let pkg = Package::default();
        assert!(!analyzer_applies_to_package(&analyzer, &pkg, &mut DepMemo::default()));
        let mut with_zap = Package::default();
        with_zap.imports.insert(
            "go.uber.org/zap".into(),
            Arc::new(Package::default()),
        );
        assert!(analyzer_applies_to_package(&analyzer, &with_zap, &mut DepMemo::default()));
        let mut with_slog = Package::default();
        with_slog
            .imports
            .insert("log/slog".into(), Arc::new(Package::default()));
        assert!(analyzer_applies_to_package(&analyzer, &with_slog, &mut DepMemo::default()));
    }

    #[test]
    fn fatcontext_skipped_without_context_import() {
        let analyzer = Analyzer {
            name: "fatcontext",
            doc: "",
            url: "",
            run: |_p| Ok(None),
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        };
        let pkg = Package::default();
        assert!(!analyzer_applies_to_package(&analyzer, &pkg, &mut DepMemo::default()));
        let mut with_ctx = Package::default();
        with_ctx
            .imports
            .insert("context".into(), Arc::new(Package::default()));
        assert!(analyzer_applies_to_package(&analyzer, &with_ctx, &mut DepMemo::default()));
    }

    #[test]
    fn lostcancel_skipped_without_context_import() {
        let analyzer = Analyzer {
            name: "lostcancel",
            doc: "",
            url: "",
            run: |_p| Ok(None),
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        };
        let pkg = Package::default();
        assert!(!analyzer_applies_to_package(&analyzer, &pkg, &mut DepMemo::default()));
        let mut with_ctx = Package::default();
        with_ctx
            .imports
            .insert("context".into(), Arc::new(Package::default()));
        assert!(analyzer_applies_to_package(&analyzer, &with_ctx, &mut DepMemo::default()));
    }

    #[test]
    fn logger_gates_accept_transitive_dependencies() {
        // A `*slog.Logger` is usually built by a helper package and handed to
        // the call site, which then never imports `log/slog` itself. Gating on
        // the direct import silently dropped every sloglint finding in an
        // http-interceptor package observed in the wild.
        for name in ["sloglint", "slog", "loggercheck"] {
            let analyzer = Analyzer {
                name,
                doc: "",
                url: "",
                run: |_p| Ok(None),
                run_despite_errors: false,
                requires: vec![],
                fact_types: vec![],
            };

            let mut unrelated = Package::default();
            unrelated
                .imports
                .insert("fmt".into(), Arc::new(Package::default()));
            unrelated.deps = vec!["fmt".into()];
            assert!(
                !analyzer_applies_to_package(&analyzer, &unrelated, &mut DepMemo::default()),
                "{name} should still skip packages with no slog anywhere"
            );

            // Direct import: kept, as before.
            let mut direct = Package::default();
            direct
                .imports
                .insert("log/slog".into(), Arc::new(Package::default()));
            assert!(analyzer_applies_to_package(&analyzer, &direct, &mut DepMemo::default()), "{name}");

            // Only transitive: must now be kept too.
            let mut transitive = Package::default();
            transitive
                .imports
                .insert("example.com/app/logging".into(), Arc::new(Package::default()));
            transitive.deps = vec!["example.com/app/logging".into(), "log/slog".into()];
            assert!(
                analyzer_applies_to_package(&analyzer, &transitive, &mut DepMemo::default()),
                "{name} must run when log/slog is only a transitive dep"
            );
        }
    }

    #[test]
    fn govet_import_gates() {
        let cases: &[(&str, &str)] = &[
            ("atomic", "sync/atomic"),
            ("SA1027", "sync/atomic"),
            ("sigchanyzer", "os/signal"),
            ("SA1017", "os/signal"),
            ("httpresponse", "net/http"),
            ("SA1020", "net/http"),
            ("defers", "time"),
            ("timeformat", "time"),
            ("SA1002", "time"),
            ("SA1004", "time"),
            ("SA1015", "time"),
            ("SA1012", "context"),
            ("SA1029", "context"),
            ("slog", "log/slog"),
            ("errorsas", "errors"),
            ("SA1032", "errors"),
            ("SA1000", "regexp"),
        ];
        for &(name, import) in cases {
            let analyzer = Analyzer {
                name,
                doc: "",
                url: "",
                run: |_p| Ok(None),
                run_despite_errors: false,
                requires: vec![],
                fact_types: vec![],
            };
            let pkg = Package::default();
            assert!(
                !analyzer_applies_to_package(&analyzer, &pkg, &mut DepMemo::default()),
                "{name} should skip without {import}"
            );
            let mut with = Package::default();
            with.imports
                .insert(import.into(), Arc::new(Package::default()));
            assert!(
                analyzer_applies_to_package(&analyzer, &with, &mut DepMemo::default()),
                "{name} should keep with {import}"
            );
        }
        let unmarshal = Analyzer {
            name: "unmarshal",
            doc: "",
            url: "",
            run: |_p| Ok(None),
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        };
        assert!(!analyzer_applies_to_package(&unmarshal, &Package::default(), &mut DepMemo::default()));
        let mut with_json = Package::default();
        with_json
            .imports
            .insert("encoding/json".into(), Arc::new(Package::default()));
        assert!(analyzer_applies_to_package(&unmarshal, &with_json, &mut DepMemo::default()));
        let cgocall = Analyzer {
            name: "cgocall",
            doc: "",
            url: "",
            run: |_p| Ok(None),
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        };
        assert!(!analyzer_applies_to_package(&cgocall, &Package::default(), &mut DepMemo::default()));
        let mut with_c = Package::default();
        with_c
            .imports
            .insert("C".into(), Arc::new(Package::default()));
        assert!(analyzer_applies_to_package(&cgocall, &with_c, &mut DepMemo::default()));
    }
}
