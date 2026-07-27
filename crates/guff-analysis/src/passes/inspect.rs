//! The `inspect` analyzer — preorder AST traversal for dependent passes.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/inspect`.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use guff::ast::File;
use guff::walk::{NodeRef, preorder_stack};

use crate::analyzer::{AnalysisResult, Analyzer, RunError, RunFn};
use crate::pass::Pass;

/// Debug-only accounting for [`InspectResult::preorder`] (PERF_TASKS_V2 B-0:
/// the GO/NO-GO measurement for the flat-Inspector rewrite).
///
/// Every dependent analyzer rewalks the whole AST and throws away the node
/// kinds it does not want, so the question B-1 hinges on is: how much CPU does
/// that rewalking actually cost, relative to the analyze phase as a whole?
///
/// Cost discipline (§B-0 「やってはいけない」):
///   * the env flag is read **once** into a `LazyLock<bool>`, never per call;
///   * `Instant::now()` is taken once per `preorder` *call*, never inside the
///     node callback — a clock read per node would dwarf what it measures;
///   * counters live in a per-thread `Arc` that is also parked in a global
///     registry, so the hot path does three *uncontended* relaxed adds and the
///     reporter can still sum across rayon workers after they go idle.
#[derive(Default)]
struct PreorderCounters {
    calls: AtomicU64,
    nodes: AtomicU64,
    nanos: AtomicU64,
}

static PREORDER_ENABLED: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("GUFF_DEBUG_CACHE").is_some());

static PREORDER_REGISTRY: LazyLock<Mutex<Vec<Arc<PreorderCounters>>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

thread_local! {
    static PREORDER_LOCAL: Arc<PreorderCounters> = {
        let c = Arc::new(PreorderCounters::default());
        PREORDER_REGISTRY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Arc::clone(&c));
        c
    };

    /// Re-entrancy depth. Some analyzers walk the AST *from inside* a preorder
    /// callback — `SA4023::interface_from_typed_nil` runs a full-file walk per
    /// candidate identifier — so the inner walk's time is already inside the
    /// outer call's `elapsed()`. Charging both would inflate the total (it made
    /// SA4023 report more preorder time than its whole analyzer run took), so
    /// only depth 0 accrues time. Calls and nodes still count every walk: they
    /// describe the work done, and the nesting is exactly what B-1 must fix.
    static PREORDER_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Increments [`PREORDER_DEPTH`] for its lifetime, restoring it even if a
/// callback panics (an analyzer panic is caught by the runner, and a leaked
/// depth would silently zero out every later measurement on that thread).
struct DepthGuard(u32);

impl DepthGuard {
    fn enter() -> Self {
        let prev = PREORDER_DEPTH.with(|d| {
            let prev = d.get();
            d.set(prev + 1);
            prev
        });
        Self(prev)
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        PREORDER_DEPTH.with(|d| d.set(self.0));
    }
}

/// Whether preorder accounting is on (`GUFF_DEBUG_CACHE` set).
pub fn preorder_stats_enabled() -> bool {
    *PREORDER_ENABLED
}

/// `(calls, nodes, nanos)` accumulated by **this thread** so far.
///
/// The runner snapshots this around each analyzer run and attributes the delta
/// to that analyzer — cheaper than threading an analyzer name into the hot path,
/// and correct because one action runs to completion on one thread.
pub fn preorder_thread_totals() -> (u64, u64, u64) {
    if !*PREORDER_ENABLED {
        return (0, 0, 0);
    }
    PREORDER_LOCAL.with(|c| {
        (
            c.calls.load(Ordering::Relaxed),
            c.nodes.load(Ordering::Relaxed),
            c.nanos.load(Ordering::Relaxed),
        )
    })
}

/// `(calls, nodes, nanos)` summed across every worker thread.
pub fn preorder_totals() -> (u64, u64, u64) {
    if !*PREORDER_ENABLED {
        return (0, 0, 0);
    }
    let reg = PREORDER_REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    reg.iter().fold((0, 0, 0), |(ca, no, na), c| {
        (
            ca + c.calls.load(Ordering::Relaxed),
            no + c.nodes.load(Ordering::Relaxed),
            na + c.nanos.load(Ordering::Relaxed),
        )
    })
}

/// Result of the `inspect` analyzer.
///
/// Simplified stand-in for Go's `ast/inspector.Inspector`. Dependent analyzers
/// call [`InspectResult::preorder`] with the same [`File`] slice from the pass.
///
/// Empty on purpose: this port rewalks on each [`preorder`] call, so collecting
/// node ids at analyzer-run time was unused overhead.
#[derive(Clone, Default)]
pub struct InspectResult {}

impl InspectResult {
    /// Visit every AST node in each file once, in preorder.
    pub fn preorder<F>(&self, files: &[File], mut f: F)
    where
        F: FnMut(NodeRef<'_>),
    {
        if *PREORDER_ENABLED {
            return self.preorder_counted(files, f);
        }
        let mut stack = Vec::new();
        for file in files {
            preorder_stack(NodeRef::File(file), &mut stack, |n, _| {
                f(n);
                true
            });
        }
    }

    /// Same walk as [`preorder`](Self::preorder), plus B-0 accounting.
    ///
    /// Split out so the default path keeps the original loop verbatim: the only
    /// cost when `GUFF_DEBUG_CACHE` is unset is one branch on a cached `bool`
    /// per call. The node counter is a plain local incremented in the callback
    /// and published once at the end — no atomics per node.
    #[cold]
    fn preorder_counted<F>(&self, files: &[File], mut f: F)
    where
        F: FnMut(NodeRef<'_>),
    {
        let guard = DepthGuard::enter();
        let start = (guard.0 == 0).then(std::time::Instant::now);
        let mut nodes: u64 = 0;
        let mut stack = Vec::new();
        for file in files {
            preorder_stack(NodeRef::File(file), &mut stack, |n, _| {
                nodes += 1;
                f(n);
                true
            });
        }
        let nanos = start.map_or(0, |s| s.elapsed().as_nanos() as u64);
        drop(guard);
        PREORDER_LOCAL.with(|c| {
            c.calls.fetch_add(1, Ordering::Relaxed);
            c.nodes.fetch_add(nodes, Ordering::Relaxed);
            c.nanos.fetch_add(nanos, Ordering::Relaxed);
        });
    }
}

fn run(_pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    Ok(Some(Box::new(InspectResult::default())))
}

fn inspect_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "inspect",
        doc: "optimize AST traversal for later passes",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/inspect",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![],
        fact_types: vec![],
    }
}

/// The `inspect` analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(inspect_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use guff::parser::{parse_file, Mode};
    use guff::position::FileSet;
    use guff::walk::preorder;

    use super::*;

    const SRC: &str = "package p\n\nfunc f(x int) int {\n\treturn x + 1\n}\n";

    #[test]
    fn inspect_preorder_visits_each_node_once() {
        let fset = FileSet::new();
        let file = parse_file(&fset, "p.go", SRC.as_bytes(), Mode::NONE).expect("parse");

        let mut first_count = 0usize;
        preorder(NodeRef::File(&file), |_| {
            first_count += 1;
            true
        });

        let result = InspectResult::default();
        let mut second_count = 0usize;
        result.preorder(std::slice::from_ref(&file), |_| {
            second_count += 1;
        });

        assert!(first_count > 5, "expected many nodes, got {first_count}");
        assert_eq!(first_count, second_count);
    }

    #[test]
    fn inspect_analyzer_validates() {
        assert!(crate::validate::validate(&[analyzer()]).is_ok());
    }
}
