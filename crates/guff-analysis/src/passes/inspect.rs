//! The `inspect` analyzer — preorder AST traversal for dependent passes.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/inspect`.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use guff::ast::File;
use guff::walk::{NodeKind, NodeMask, NodeRef, preorder_stack};
use guff_packages::Package;

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
    /// Events *scanned*. This is the traversal work, and it stays the same
    /// whether or not the caller passed a mask — until B-1d lets a masked scan
    /// jump over whole subtrees, at which point this is the number that drops.
    nodes: AtomicU64,
    /// Events *delivered* to the callback. `nodes - hits` is what B-1c's masks
    /// have taken off the callback path so far.
    hits: AtomicU64,
    nanos: AtomicU64,
}

static PREORDER_ENABLED: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("GUFF_DEBUG_CACHE").is_some());

/// Escape hatch for the B-1c migration: `GUFF_INSPECT_MASKS=0` makes every
/// [`InspectResult::preorder_typed`] call behave as if it had asked for
/// [`NodeMask::ALL`].
///
/// A mask that omits a kind its callback handles does not fail loudly — the
/// analyzer just stops finding things, which is the worst kind of regression
/// and invisible on a corpus where that analyzer happens to be silent. With
/// this switch the check is a single binary run twice over any corpus: masks on
/// vs masks off must produce byte-identical findings. Read once into a
/// `LazyLock`, so the cost is one branch on a cached `bool` per *call*.
static MASKS_ENABLED: LazyLock<bool> =
    LazyLock::new(|| std::env::var("GUFF_INSPECT_MASKS").as_deref() != Ok("0"));

/// Whether node-kind masks are honoured (`GUFF_INSPECT_MASKS` is not `0`).
pub fn masks_enabled() -> bool {
    *MASKS_ENABLED
}

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
    /// callback, so the inner walk's time is already inside the outer call's
    /// `elapsed()`. Charging both would inflate the total (it made SA4023 report
    /// more preorder time than its whole analyzer run took), so only depth 0
    /// accrues time. Calls and nodes still count every walk: they describe the
    /// work done, and the nesting is exactly what B-1 must fix.
    ///
    /// SA4023 was the example here, running a full-file walk *per candidate
    /// identifier*: 267M of the run's 467M scanned nodes on prometheus `./...`.
    /// It now builds one index lazily (`concrete_pointer_assigns`), so it still
    /// nests — once — and this guard still covers it.
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

/// `(calls, nodes, nanos, hits)` accumulated by **this thread** so far.
///
/// The runner snapshots this around each analyzer run and attributes the delta
/// to that analyzer — cheaper than threading an analyzer name into the hot path,
/// and correct because one action runs to completion on one thread.
pub fn preorder_thread_totals() -> (u64, u64, u64, u64) {
    if !*PREORDER_ENABLED {
        return (0, 0, 0, 0);
    }
    PREORDER_LOCAL.with(|c| {
        (
            c.calls.load(Ordering::Relaxed),
            c.nodes.load(Ordering::Relaxed),
            c.nanos.load(Ordering::Relaxed),
            c.hits.load(Ordering::Relaxed),
        )
    })
}

/// `(calls, nodes, nanos, hits)` summed across every worker thread.
pub fn preorder_totals() -> (u64, u64, u64, u64) {
    if !*PREORDER_ENABLED {
        return (0, 0, 0, 0);
    }
    let reg = PREORDER_REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    reg.iter().fold((0, 0, 0, 0), |(ca, no, na, hi), c| {
        (
            ca + c.calls.load(Ordering::Relaxed),
            no + c.nodes.load(Ordering::Relaxed),
            na + c.nanos.load(Ordering::Relaxed),
            hi + c.hits.load(Ordering::Relaxed),
        )
    })
}

/// One visited node, flattened out of the AST (PERF_TASKS_V2 B-1a).
///
/// Go's `inspector` stores `[]ast.Node` — an interface value that already
/// carries the concrete type. `NodeRef<'a>` borrows, so it cannot live in a
/// `'static` `AnalysisResult`; the equivalent here is the kind plus a
/// type-erased pointer, which round-trips through `NodeRef::from_erased`.
#[derive(Clone, Copy)]
struct Event {
    ptr: *const (),
    kind: NodeKind,
}

/// The preorder sequence of one package's files, built once.
struct Events {
    /// Nodes in exactly the order [`preorder_stack`] would visit them.
    nodes: Vec<Event>,
    /// Identity of the `&[File]` the events came from. **Compared, never
    /// dereferenced.** A caller that hands `preorder` some other slice (a
    /// single file, a filtered list) falls back to walking it.
    files_ptr: *const File,
    files_len: usize,
    /// Keeps that AST alive for as long as any clone of this result exists.
    ///
    /// Without it, `from_erased` would be sound only because of how the runner
    /// happens to order `release_finished_deps` against dropping the `Action`
    /// that owns the package. With it, the guarantee is local and cannot be
    /// invalidated from outside this file.
    _owner: Arc<Package>,
}

// SAFETY: `Events` is immutable once built, and its raw pointers are only ever
// read through `&`. They point into the AST owned by `_owner`, and
// `Arc<Package>` is already shared across rayon workers by the runner — so
// sharing these pointers is exactly as safe as sharing that `&Package`.
unsafe impl Send for Events {}
unsafe impl Sync for Events {}

/// Rebuild a node reference, tying its lifetime to the caller's `files`.
///
/// # Safety
///
/// `e` must have been recorded from a node inside `files` (checked by
/// [`InspectResult::events_for`] before this is reached).
#[inline]
unsafe fn node_in<'a>(files: &'a [File], e: Event) -> NodeRef<'a> {
    // Binds the returned lifetime to the borrow of `files` rather than letting
    // it be inferred as anything the callback would accept.
    let _ = files;
    unsafe { NodeRef::from_erased(e.kind, e.ptr) }
}

/// Result of the `inspect` analyzer.
///
/// Port of Go's `ast/inspector.Inspector`: the package's AST is flattened once,
/// and each dependent analyzer scans that array instead of re-walking the tree.
/// Dependent analyzers call [`InspectResult::preorder`] with the same [`File`]
/// slice from the pass.
#[derive(Clone, Default)]
pub struct InspectResult {
    /// `None` when the pass gave no owning package handle (tests, ad-hoc
    /// passes). Every entry point degrades to the recursive walk.
    events: Option<Arc<Events>>,
}

impl InspectResult {
    /// Flatten `pass`'s files, or produce a walk-only result if the pass has no
    /// owning `Arc<Package>` to anchor the pointers to.
    fn build(pass: &Pass<'_>) -> Self {
        let Some(owner) = pass.pkg_arc().cloned() else {
            return Self::default();
        };
        let files = pass.files();
        let mut nodes = Vec::new();
        let mut stack = Vec::new();
        for file in files {
            preorder_stack(NodeRef::File(file), &mut stack, |n, _| {
                nodes.push(Event {
                    ptr: n.erased_ptr(),
                    kind: n.kind(),
                });
                true
            });
        }
        nodes.shrink_to_fit();
        Self {
            events: Some(Arc::new(Events {
                nodes,
                files_ptr: files.as_ptr(),
                files_len: files.len(),
                _owner: owner,
            })),
        }
    }

    /// The flattened events, but only if `files` is the very slice they were
    /// built from.
    #[inline]
    fn events_for(&self, files: &[File]) -> Option<&[Event]> {
        let ev = self.events.as_ref()?;
        (ev.files_ptr == files.as_ptr() && ev.files_len == files.len())
            .then(|| ev.nodes.as_slice())
    }

    /// Visit every AST node in each file once, in preorder.
    pub fn preorder<F>(&self, files: &[File], f: F)
    where
        F: FnMut(NodeRef<'_>),
    {
        self.preorder_typed(NodeMask::ALL, files, f);
    }

    /// Visit only the nodes whose kind is in `mask`, in preorder.
    ///
    /// Port of Go's `inspector.Preorder(types, f)` (PERF_TASKS_V2 B-1b). The
    /// sequence is exactly `preorder` filtered by `mask` — same nodes, same
    /// order — so a caller that already discarded other kinds itself (the
    /// `let NodeRef::X(x) = n else { return }` shape at ~144 call sites) can
    /// pass a mask and get identical behaviour without the per-node callback.
    ///
    /// **Keep the `else { return }` when migrating.** A mask that is missing a
    /// kind the body handles silently drops findings; `unreachable!()` would
    /// turn that into a crash but does not make the mask any more correct, and
    /// the discarded-kind branch costs nothing once the mask filters it out.
    ///
    /// Setting `GUFF_INSPECT_MASKS=0` widens every mask back to
    /// [`NodeMask::ALL`], which is how a migration is checked: one binary, two
    /// runs, and any silently-narrow mask anywhere shows up as a findings diff
    /// on real code. See [`masks_enabled`].
    pub fn preorder_typed<F>(&self, mask: NodeMask, files: &[File], f: F)
    where
        F: FnMut(NodeRef<'_>),
    {
        let mask = if *MASKS_ENABLED { mask } else { NodeMask::ALL };
        if *PREORDER_ENABLED {
            return self.preorder_counted(mask, files, f);
        }
        self.visit_masked(mask, files, f);
    }

    /// The traversal itself: a linear scan when the events match, the original
    /// recursive walk otherwise. Both produce the identical node sequence — the
    /// events were recorded by the same [`preorder_stack`] call shape.
    ///
    /// Returns how many nodes were scanned and how many were delivered; both
    /// are dropped on the release path (the counters are only read under
    /// `GUFF_DEBUG_CACHE`) and exist so the two arms can't disagree about what
    /// counts as work.
    #[inline]
    fn visit_masked<F>(&self, mask: NodeMask, files: &[File], mut f: F) -> (u64, u64)
    where
        F: FnMut(NodeRef<'_>),
    {
        let mut scanned: u64 = 0;
        let mut hits: u64 = 0;
        match self.events_for(files) {
            Some(events) => {
                scanned = events.len() as u64;
                for &e in events {
                    if !mask.contains(e.kind) {
                        continue;
                    }
                    hits += 1;
                    // SAFETY: `events_for` just confirmed these events were
                    // recorded from this exact slice, and `Events::_owner` keeps
                    // its AST alive; the nodes are therefore live and unmoved.
                    f(unsafe { node_in(files, e) });
                }
            }
            None => {
                let mut stack = Vec::new();
                for file in files {
                    preorder_stack(NodeRef::File(file), &mut stack, |n, _| {
                        scanned += 1;
                        if mask.contains(n.kind()) {
                            hits += 1;
                            f(n);
                        }
                        true
                    });
                }
            }
        }
        (scanned, hits)
    }

    /// Same traversal as [`preorder_typed`](Self::preorder_typed), plus B-0
    /// accounting.
    ///
    /// Split out so the default path costs one branch on a cached `bool` per
    /// call. The counters are plain locals published once at the end — no
    /// atomics per node.
    #[cold]
    fn preorder_counted<F>(&self, mask: NodeMask, files: &[File], f: F)
    where
        F: FnMut(NodeRef<'_>),
    {
        let guard = DepthGuard::enter();
        let start = (guard.0 == 0).then(std::time::Instant::now);
        let (scanned, hits) = self.visit_masked(mask, files, f);
        let nanos = start.map_or(0, |s| s.elapsed().as_nanos() as u64);
        drop(guard);
        PREORDER_LOCAL.with(|c| {
            c.calls.fetch_add(1, Ordering::Relaxed);
            c.nodes.fetch_add(scanned, Ordering::Relaxed);
            c.hits.fetch_add(hits, Ordering::Relaxed);
            c.nanos.fetch_add(nanos, Ordering::Relaxed);
        });
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    Ok(Some(Box::new(InspectResult::build(pass))))
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
    use guff::node_mask;
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

    /// Two files, so the flat path has to concatenate per-file walks the same
    /// way the recursive one does.
    const SRC2: &str = "package p\n\nvar v = map[string][]int{\"a\": {1, 2}}\n\n\
                        type T struct{ X int }\n\n\
                        func (t *T) M() error { defer func() {}(); return nil }\n";

    fn flat_result(pkg: &Arc<Package>, fset: &Arc<guff::position::FileSet>) -> InspectResult {
        let mut diags = Vec::new();
        let mut facts = crate::facts::FactStore::default();
        let mut pass = crate::pass::PassInput {
            analyzer: analyzer(),
            fset,
            files: &pkg.syntax,
            pkg,
            pkg_arc: Some(Arc::clone(pkg)),
            types_info: None,
            types_sizes: guff_types::default_sizes(),
            diagnostics: &mut diags,
            result_of: std::collections::HashMap::new(),
            facts: &mut facts,
            settings: Arc::new(crate::SettingsBag::default()),
        }
        .build();
        let result = run(&mut pass).expect("run").expect("result");
        result
            .downcast_ref::<InspectResult>()
            .expect("InspectResult")
            .clone()
    }

    fn two_file_package() -> (Arc<Package>, Arc<guff::position::FileSet>) {
        let fset = FileSet::new();
        let a = parse_file(&fset, "a.go", SRC.as_bytes(), Mode::NONE).expect("parse a");
        let b = parse_file(&fset, "b.go", SRC2.as_bytes(), Mode::NONE).expect("parse b");
        let pkg = Package {
            id: "p".into(),
            syntax: vec![a, b],
            ..Package::default()
        };
        (Arc::new(pkg), fset)
    }

    /// The flat event array must reproduce the recursive walk **exactly** —
    /// same nodes, same order, same identity. A silently shorter sequence is
    /// how B-1 would drop findings.
    #[test]
    fn flat_events_match_the_recursive_walk() {
        let (pkg, fset) = two_file_package();
        let flat = flat_result(&pkg, &fset);
        assert!(flat.events.is_some(), "expected the flat path to be built");

        let mut expected: Vec<(&'static str, *const ())> = Vec::new();
        for file in &pkg.syntax {
            preorder(NodeRef::File(file), |n| {
                expected.push((n.kind_name(), n.erased_ptr()));
                true
            });
        }

        let mut got: Vec<(&'static str, *const ())> = Vec::new();
        flat.preorder(&pkg.syntax, |n| got.push((n.kind_name(), n.erased_ptr())));

        assert!(expected.len() > 40, "expected many nodes, got {}", expected.len());
        assert_eq!(expected, got);
    }

    /// A caller that passes some other slice must not be served the cached
    /// events; it gets a real walk of what it actually handed us.
    #[test]
    fn foreign_file_slice_falls_back_to_walking() {
        let (pkg, fset) = two_file_package();
        let flat = flat_result(&pkg, &fset);

        let only_first = std::slice::from_ref(&pkg.syntax[0]);
        assert!(flat.events_for(only_first).is_none());

        let mut whole = 0usize;
        flat.preorder(&pkg.syntax, |_| whole += 1);
        let mut first = 0usize;
        flat.preorder(only_first, |_| first += 1);

        let mut direct = 0usize;
        preorder(NodeRef::File(&pkg.syntax[0]), |_| {
            direct += 1;
            true
        });
        assert_eq!(first, direct);
        assert!(first < whole, "subset must visit fewer nodes ({first} vs {whole})");
    }

    /// A masked traversal must be **exactly** the unmasked one filtered — same
    /// nodes, same order — on both the flat and the fallback path. If the two
    /// disagreed, migrating a call site to a mask would change findings.
    #[test]
    fn masked_preorder_is_the_unmasked_sequence_filtered() {
        let (pkg, fset) = two_file_package();
        let flat = flat_result(&pkg, &fset);
        let mask = node_mask!(Ident, CallExpr, FuncDecl);

        let mut all: Vec<(&'static str, *const ())> = Vec::new();
        flat.preorder(&pkg.syntax, |n| all.push((n.kind_name(), n.erased_ptr())));
        let expected: Vec<_> = all
            .iter()
            .copied()
            .filter(|(k, _)| matches!(*k, "Ident" | "CallExpr" | "FuncDecl"))
            .collect();
        assert!(!expected.is_empty(), "fixture must contain those kinds");
        assert!(expected.len() < all.len(), "mask must filter something out");

        let mut got: Vec<(&'static str, *const ())> = Vec::new();
        flat.preorder_typed(mask, &pkg.syntax, |n| {
            got.push((n.kind_name(), n.erased_ptr()));
        });
        assert_eq!(expected, got, "flat masked scan");

        // The fallback path (a slice the events weren't built from) has to
        // filter identically — it is a separate arm of `visit_masked`.
        let one = std::slice::from_ref(&pkg.syntax[0]);
        let mut fb_all: Vec<(&'static str, *const ())> = Vec::new();
        flat.preorder(one, |n| fb_all.push((n.kind_name(), n.erased_ptr())));
        let fb_expected: Vec<_> = fb_all
            .iter()
            .copied()
            .filter(|(k, _)| matches!(*k, "Ident" | "CallExpr" | "FuncDecl"))
            .collect();
        let mut fb_got: Vec<(&'static str, *const ())> = Vec::new();
        flat.preorder_typed(mask, one, |n| fb_got.push((n.kind_name(), n.erased_ptr())));
        assert_eq!(fb_expected, fb_got, "fallback masked walk");
    }

    /// `NodeMask::ALL` is generated from the same variant list as `NodeKind`,
    /// so `preorder` and `preorder_typed(ALL, ..)` must not diverge — that
    /// equality is what makes every unmigrated call site correct.
    #[test]
    fn all_mask_matches_every_kind() {
        let (pkg, fset) = two_file_package();
        let flat = flat_result(&pkg, &fset);

        let mut plain = 0usize;
        flat.preorder(&pkg.syntax, |n| {
            assert!(NodeMask::ALL.contains(n.kind()), "{} missing", n.kind_name());
            plain += 1;
        });
        let mut masked = 0usize;
        flat.preorder_typed(NodeMask::ALL, &pkg.syntax, |_| masked += 1);
        assert_eq!(plain, masked);

        assert!(NodeMask::NONE.is_empty());
        let mut none = 0usize;
        flat.preorder_typed(NodeMask::NONE, &pkg.syntax, |_| none += 1);
        assert_eq!(none, 0, "the empty mask must deliver nothing");
    }

    /// Without an owning package handle there is nothing to anchor raw pointers
    /// to, so `build` must decline and every call must still walk correctly.
    #[test]
    fn no_owner_means_no_events() {
        let empty = InspectResult::default();
        assert!(empty.events.is_none());

        let fset = FileSet::new();
        let file = parse_file(&fset, "p.go", SRC.as_bytes(), Mode::NONE).expect("parse");
        let files = std::slice::from_ref(&file);

        let mut walked = 0usize;
        empty.preorder(files, |_| walked += 1);
        let mut direct = 0usize;
        preorder(NodeRef::File(&file), |_| {
            direct += 1;
            true
        });
        assert_eq!(walked, direct);
    }
}
