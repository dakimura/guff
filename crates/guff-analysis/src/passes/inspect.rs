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
    /// Which arm of [`InspectResult::visit_masked`] each call took, indexed by
    /// [`Arm`] (PERF_TASKS_V8 §1 / P0).
    ///
    /// V1-2 built the per-kind groups so that a one-kind mask would deliver
    /// `O(hits)` instead of `O(all nodes)`, but the run-wide counters say the
    /// opposite is happening: prometheus `./...` scans 198M events to deliver
    /// 6.75M, and a single 4,385-node package is scanned 126 times over. Both
    /// fast arms set `hits == scanned`, so neither can be producing those
    /// numbers — something is falling through to a slower arm, and `scanned`
    /// alone cannot say which. This counts the arms directly.
    arms: [AtomicU64; Arm::COUNT],
}

/// Which branch of [`InspectResult::visit_masked`] served a call.
///
/// The two `Walk*` variants are the same code path (the recursive fallback);
/// they are counted apart because they have completely different fixes — one is
/// a missing `Arc<Package>` on the pass, the other is a caller passing a `File`
/// slice the events were not built from.
#[derive(Clone, Copy)]
#[repr(u8)]
enum Arm {
    /// One kind: its group *is* the answer. `hits == scanned`.
    Single = 0,
    /// 2..=`MAX_MERGE_KINDS` kinds, merged by event index. `hits == scanned`.
    Merge = 1,
    /// Wider than `MAX_MERGE_KINDS`: linear scan of the event window.
    Wide = 2,
    /// Recursive walk because the result carries no events at all — the pass
    /// had no `Arc<Package>` for [`InspectResult::build`] to anchor to.
    WalkNoEvents = 3,
    /// Recursive walk because `files` is not the slice the events were built
    /// from, nor a contiguous subslice of it.
    WalkForeignSlice = 4,
}

impl Arm {
    const COUNT: usize = 5;

    const NAMES: [&'static str; Self::COUNT] = [
        "single-kind group",
        "merged groups",
        "wide linear scan",
        "recursive walk (no events)",
        "recursive walk (foreign slice)",
    ];
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

/// `(arm name, calls)` for each [`Arm`], summed across every worker thread.
///
/// Answers "is the V1-2 fast path being taken at all?" without inferring it
/// from `scanned` vs `delivered`, which cannot tell the wide scan apart from
/// the recursive walk.
pub fn preorder_arm_totals() -> Vec<(&'static str, u64)> {
    if !*PREORDER_ENABLED {
        return Vec::new();
    }
    let reg = PREORDER_REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = [0u64; Arm::COUNT];
    for c in reg.iter() {
        for (slot, a) in out.iter_mut().zip(c.arms.iter()) {
            *slot += a.load(Ordering::Relaxed);
        }
    }
    Arm::NAMES.iter().copied().zip(out).collect()
}

/// The preorder sequence of one package's files, built once.
///
/// One visited node is a [`NodeKind`] plus a type-erased pointer (PERF_TASKS_V2
/// B-1a). Go's `inspector` stores `[]ast.Node`, an interface value that already
/// carries its concrete type; `NodeRef<'a>` borrows, so it cannot live in a
/// `'static` `AnalysisResult`, and the pair round-trips through
/// `NodeRef::from_erased` instead.
///
/// The pair is stored as **two arrays, not an array of pairs** (PERF_TASKS_V8
/// §V8-3). A struct of `{*const (), NodeKind}` is 16 bytes — 8 of pointer, 1 of
/// kind, 7 of padding — and the wide linear scan reads only the kind. Split, it
/// streams 1 byte per node instead of 16 on the arm that scans the most nodes,
/// and the whole structure gets 44% smaller.
struct Events {
    /// The erased pointer of each node, in exactly the order [`preorder_stack`]
    /// would visit them.
    ptrs: Vec<*const ()>,
    /// The kind of each node, at the same index as [`Self::ptrs`].
    kinds: Vec<NodeKind>,
    /// Indices into [`Self::ptrs`], grouped by [`NodeKind`] (PERF_TASKS_V3
    /// V1-2). Group `k` is `by_kind[kind_off[k] .. kind_off[k + 1]]`, and its
    /// entries ascend — so iterating a group yields that kind's nodes in
    /// preorder order, without touching the other 97%.
    ///
    /// The linear scan this replaces read every event and threw away the ones
    /// the mask did not want. On prometheus `./...` that was 202M events
    /// scanned to deliver 7M (measured with `GUFF_DEBUG_CACHE=2`), because 119
    /// of the 149 `preorder_typed` call sites ask for exactly **one** kind and
    /// 15 more ask for two.
    by_kind: Vec<u32>,
    /// Start offset of each kind's group in [`Self::by_kind`], plus a terminator.
    kind_off: [u32; NodeKind::COUNT + 1],
    /// Where each file's events begin in [`Self::ptrs`], plus a terminator
    /// (length `files_len + 1`).
    ///
    /// Files were flattened in order, so file `i` owns
    /// `ptrs[file_off[i] .. file_off[i + 1]]` — a contiguous run. That is what
    /// lets a caller passing `std::slice::from_ref(file)` still use the flat
    /// index: five call sites do that (S1008, S1002, …), and before
    /// PERF_TASKS_V3 V1-2b they fell all the way back to a fresh recursive walk
    /// of the file. S1008 was the most expensive analyzer in the run.
    file_off: Vec<u32>,
    /// Identity of the `&[File]` the events came from. **Compared and used for
    /// offset arithmetic, never dereferenced.**
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

/// Counting-sort event indices into per-kind groups (PERF_TASKS_V3 V1-2).
///
/// Returns `(by_kind, kind_off)` where group `k` occupies
/// `by_kind[kind_off[k] .. kind_off[k + 1]]`. Two linear passes: count, then
/// scatter. Because the scatter walks `kinds` front to back, each group comes
/// out in ascending index order — which is preorder order, so a masked walk
/// over a group is indistinguishable from the filtered linear scan.
fn index_by_kind(kinds: &[NodeKind]) -> (Vec<u32>, [u32; NodeKind::COUNT + 1]) {
    let mut off = [0u32; NodeKind::COUNT + 1];
    for &k in kinds {
        off[k as usize + 1] += 1;
    }
    for k in 0..NodeKind::COUNT {
        off[k + 1] += off[k];
    }
    let mut cursor = off;
    let mut by_kind = vec![0u32; kinds.len()];
    for (i, &k) in kinds.iter().enumerate() {
        let slot = &mut cursor[k as usize];
        by_kind[*slot as usize] = i as u32;
        *slot += 1;
    }
    (by_kind, off)
}

/// How many kinds the cheap linear-min merge handles before the arm is chosen
/// by measurement instead of by kind count.
///
/// 137 of the 149 `preorder_typed` call sites pass three kinds or fewer. Up to
/// this many cursors, picking the minimum by scanning them all is a handful of
/// register comparisons per delivered node and always beats reading the whole
/// window — no arithmetic needed to know that.
///
/// Above it, kind count alone is the wrong question, and asking it was what
/// dropped `errcheck` (6 kinds, 5% of the window) into the same arm as
/// `gocritic` (20+ kinds, 42% of it). [`merge_beats_scan`] decides those.
const MAX_MERGE_KINDS: u32 = 4;

/// Whether merging `k` per-kind groups holding `selected` events is cheaper
/// than one mask test per event over a `total`-event window (PERF_TASKS_V8
/// §V8-3).
///
/// Both sides are counted in "units of work the arm does per node it touches":
///
///   * merge delivers `selected` nodes, and each one costs a min-select over
///     the `k` live cursors plus a random read into `ptrs` — call it `k`;
///   * scan touches `total` kinds, one byte each, sequentially, and a mask test
///     is a shift and an and. That is the cheapest thing in this file, and
///     `SCAN_UNITS_PER_NODE` being below 1 is what says so.
///
/// The numbers that made this necessary, from `GUFF_DEBUG_CACHE=2` on
/// prometheus `./...`:
///
/// | analyzer | k | selected | total | merge | scan | cheaper |
/// |---|---:|---:|---:|---:|---:|---|
/// | `errcheck` | 6 | 84,173 | 1,606,518 | 505k | 803k | merge |
/// | `copylocks` | 8 | 238,189 | 1,606,518 | 1.91M | 803k | scan |
/// | `gocritic` | 20+ | 625,288 | 1,497,576 | 12.5M | 749k | scan |
///
/// so raising [`MAX_MERGE_KINDS`] to cover `errcheck` would have dragged
/// `gocritic` along and made it 17x worse. That is why the existing comment
/// said raising it "is a measurement, not a guess" — this is the measurement.
#[inline]
fn merge_beats_scan(k: usize, selected: usize, total: usize) -> bool {
    /// How many scanned nodes cost what one merged node costs. Streaming a
    /// `u8` and testing a bit is cheaper than a cursor min-select plus a random
    /// read into `ptrs`; 2 puts the cross-over just past `copylocks` (k=8, 15%
    /// selectivity), which the V8-3 table has as a near-tie.
    const SCAN_NODES_PER_MERGE_UNIT: usize = 2;
    selected.saturating_mul(k).saturating_mul(SCAN_NODES_PER_MERGE_UNIT) < total
}

/// Rebuild a node reference, tying its lifetime to the caller's `files`.
///
/// # Safety
///
/// `kind` / `ptr` must have been recorded from a node inside `files` (checked
/// by [`InspectResult::events_for`] before this is reached), and must be the
/// pair recorded at the *same* event index — the two arrays are only a node
/// when read together.
#[inline]
unsafe fn node_in<'a>(files: &'a [File], kind: NodeKind, ptr: *const ()) -> NodeRef<'a> {
    // Binds the returned lifetime to the borrow of `files` rather than letting
    // it be inferred as anything the callback would accept.
    let _ = files;
    unsafe { NodeRef::from_erased(kind, ptr) }
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
        let mut ptrs = Vec::new();
        let mut kinds = Vec::new();
        let mut stack = Vec::new();
        let mut file_off = Vec::with_capacity(files.len() + 1);
        for file in files {
            file_off.push(ptrs.len() as u32);
            preorder_stack(NodeRef::File(file), &mut stack, |n, _| {
                ptrs.push(n.erased_ptr());
                kinds.push(n.kind());
                true
            });
        }
        file_off.push(ptrs.len() as u32);
        ptrs.shrink_to_fit();
        kinds.shrink_to_fit();
        let (by_kind, kind_off) = index_by_kind(&kinds);
        Self {
            events: Some(Arc::new(Events {
                ptrs,
                kinds,
                by_kind,
                kind_off,
                file_off,
                files_ptr: files.as_ptr(),
                files_len: files.len(),
                _owner: owner,
            })),
        }
    }

    /// The flattened events plus the `[start, end)` event range covering
    /// `files` — if `files` is the slice they were built from, or a contiguous
    /// subslice of it.
    ///
    /// The subslice case is what `std::slice::from_ref(&pass.files()[i])`
    /// produces, and it is common enough to matter (PERF_TASKS_V3 V1-2b).
    /// Anything else — a caller's own `Vec<File>`, a filtered list — has no
    /// events here and falls back to walking.
    #[inline]
    fn events_for(&self, files: &[File]) -> Option<(&Events, u32, u32)> {
        let ev = self.events.as_ref()?;
        if ev.files_ptr == files.as_ptr() && ev.files_len == files.len() {
            return Some((ev, 0, ev.ptrs.len() as u32));
        }
        // Address arithmetic on `usize`, never `offset_from` — the two pointers
        // are only known to share an allocation *after* the checks below, which
        // is exactly what `offset_from` would require up front.
        //
        // Why a foreign slice cannot be mistaken for one of ours: the three
        // checks together confine `here` to
        // `[base, base + files_len * stride)`, stride-aligned. That interval is
        // precisely the `[File]` the events were built from, and no other live
        // object can overlap an allocation that is still borrowed here — so a
        // pointer landing inside it *is* an element of that slice. A caller's
        // own `Vec<File>` fails the range check and falls back to walking.
        //
        // A zero-length `files` is the one case where the pointer carries no
        // such guarantee, but it also cannot go wrong: whichever branch it
        // takes, `file_off[i] .. file_off[i + 0]` is empty and the fallback
        // walk visits nothing.
        let base = ev.files_ptr as usize;
        let here = files.as_ptr() as usize;
        let stride = std::mem::size_of::<File>();
        if stride == 0 || here < base {
            return None;
        }
        let byte_delta = here - base;
        if byte_delta % stride != 0 {
            return None;
        }
        let i = byte_delta / stride;
        if i >= ev.files_len || i + files.len() > ev.files_len {
            return None;
        }
        Some((ev, ev.file_off[i], ev.file_off[i + files.len()]))
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

    /// The traversal itself: per-kind groups when the mask is narrow, a linear
    /// scan when it is wide, and the original recursive walk when the events do
    /// not belong to `files`. All three produce the identical node sequence —
    /// the events were recorded by the same [`preorder_stack`] call shape, and
    /// each group is in ascending event order.
    ///
    /// Returns how many nodes were scanned, how many were delivered, and which
    /// [`Arm`] served the call; all three are dropped on the release path (the
    /// counters are only read under `GUFF_DEBUG_CACHE`) and exist so the arms
    /// can't disagree about what counts as work.
    #[inline]
    fn visit_masked<F>(&self, mask: NodeMask, files: &[File], mut f: F) -> (u64, u64, Arm)
    where
        F: FnMut(NodeRef<'_>),
    {
        let mut scanned: u64 = 0;
        let mut hits: u64 = 0;
        let mut arm;
        match self.events_for(files) {
            Some((ev, lo, hi)) => {
                // Each kind's group is ascending, and `[lo, hi)` is a contiguous
                // run of event indices, so the group's entries for this range are
                // a contiguous slice of the group — found by binary search.
                let clip = |k: usize| -> &[u32] {
                    let g = &ev.by_kind[ev.kind_off[k] as usize..ev.kind_off[k + 1] as usize];
                    let s = g.partition_point(|&i| i < lo);
                    let e = g.partition_point(|&i| i < hi);
                    &g[s..e]
                };
                // `NodeKind::bit()` is `1 << (kind as u8)`, so a set bit's
                // position *is* the kind's discriminant — and its index into
                // `kind_off`. No `NodeKind` round-trip needed.
                let bits = mask.bits();
                let n = bits.count_ones() as usize;
                if n == 1 {
                    arm = Arm::Single;
                    // The 119-call-site case: one kind, so its group *is* the
                    // answer — already in preorder order, no merge, no mask test.
                    let k = bits.trailing_zeros() as usize;
                    let group = clip(k);
                    scanned = group.len() as u64;
                    hits = scanned;
                    // Every node in a single-kind group has that kind, so it is
                    // read once here instead of once per delivered node.
                    // `NodeMask` bits *are* discriminants, so this cannot miss;
                    // an empty group makes the `else` unreachable anyway.
                    let kind = NodeKind::from_index(k as u8);
                    // SAFETY (this loop and the two below): `events_for` just
                    // confirmed these events were recorded from this slice (or
                    // the slice it is part of), and `Events::_owner` keeps that
                    // AST alive, so the nodes are live and unmoved. Each index
                    // reads `kinds` and `ptrs` at the *same* position, which is
                    // what makes the pair a node.
                    for &idx in group {
                        let idx = idx as usize;
                        let kind = kind.unwrap_or(ev.kinds[idx]);
                        f(unsafe { node_in(files, kind, ev.ptrs[idx]) });
                    }
                } else {
                    // Cursors for every kind in the mask. Sized for the widest
                    // possible mask so the merge arm is not capped by the array:
                    // 64 slices is 1 KiB of stack, once per call, and only the
                    // first `n` are ever touched.
                    let mut cursors: [&[u32]; NodeKind::COUNT] = [&[]; NodeKind::COUNT];
                    let mut rest = bits;
                    let mut selected = 0usize;
                    for slot in cursors[..n].iter_mut() {
                        let k = rest.trailing_zeros() as usize;
                        rest &= rest - 1;
                        *slot = clip(k);
                        selected += slot.len();
                    }
                    let total = (hi - lo) as usize;
                    // Up to `MAX_MERGE_KINDS` the merge always wins; past it,
                    // the groups' real sizes decide (PERF_TASKS_V8 §V8-3). Both
                    // are `O(k)` to have: `clip` is a pair of binary searches
                    // the merge arm needs anyway.
                    if n as u32 <= MAX_MERGE_KINDS || merge_beats_scan(n, selected, total) {
                        arm = Arm::Merge;
                        // Walk only the requested kinds' groups, merging by
                        // event index so the caller still sees preorder order.
                        // `scanned` counts what we actually touched, which is
                        // the point of the whole exercise.
                        loop {
                            // Pick the group whose next event comes first.
                            let mut best: Option<(usize, u32)> = None;
                            for (i, g) in cursors[..n].iter().enumerate() {
                                let Some(&idx) = g.first() else { continue };
                                if best.is_none_or(|(_, b)| idx < b) {
                                    best = Some((i, idx));
                                }
                            }
                            let Some((slot, idx)) = best else { break };
                            cursors[slot] = &cursors[slot][1..];
                            scanned += 1;
                            hits += 1;
                            let idx = idx as usize;
                            let (kind, ptr) = (ev.kinds[idx], ev.ptrs[idx]);
                            f(unsafe { node_in(files, kind, ptr) });
                        }
                    } else {
                        arm = Arm::Wide;
                        scanned = total as u64;
                        // Only `kinds` is streamed — one byte per node, and the
                        // reason it is stored apart from `ptrs`. `ptrs` is
                        // touched once per *delivered* node.
                        let window = &ev.kinds[lo as usize..hi as usize];
                        for (i, &kind) in window.iter().enumerate() {
                            if !mask.contains(kind) {
                                continue;
                            }
                            hits += 1;
                            let ptr = ev.ptrs[lo as usize + i];
                            f(unsafe { node_in(files, kind, ptr) });
                        }
                    }
                }
            }
            None => {
                // Split the one fallback into its two causes: no events at all
                // (the pass had no `Arc<Package>`) versus events that belong to
                // a different `[File]` than the caller passed.
                arm = if self.events.is_none() {
                    Arm::WalkNoEvents
                } else {
                    Arm::WalkForeignSlice
                };
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
        (scanned, hits, arm)
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
        let (scanned, hits, arm) = self.visit_masked(mask, files, f);
        let nanos = start.map_or(0, |s| s.elapsed().as_nanos() as u64);
        drop(guard);
        PREORDER_LOCAL.with(|c| {
            c.calls.fetch_add(1, Ordering::Relaxed);
            c.nodes.fetch_add(scanned, Ordering::Relaxed);
            c.hits.fetch_add(hits, Ordering::Relaxed);
            c.nanos.fetch_add(nanos, Ordering::Relaxed);
            c.arms[arm as usize].fetch_add(1, Ordering::Relaxed);
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

    /// A single-file subslice — `std::slice::from_ref(&pass.files()[i])`, which
    /// five call sites use — is served from the flat index, clipped to that
    /// file's event range, and yields exactly the file's own nodes in order.
    ///
    /// This used to fall back to a fresh recursive walk (PERF_TASKS_V3 V1-2b);
    /// S1008 was the most expensive analyzer in the run because of it.
    #[test]
    fn single_file_subslice_uses_the_flat_index() {
        let (pkg, fset) = two_file_package();
        let flat = flat_result(&pkg, &fset);

        for i in 0..pkg.syntax.len() {
            let one = std::slice::from_ref(&pkg.syntax[i]);
            let (_, lo, hi) = flat
                .events_for(one)
                .expect("a subslice of the built files must resolve to an event range");
            assert!(lo < hi, "file {i} must own a non-empty event range");

            let mut expected: Vec<(&'static str, *const ())> = Vec::new();
            preorder(NodeRef::File(&pkg.syntax[i]), |n| {
                expected.push((n.kind_name(), n.erased_ptr()));
                true
            });
            let mut got: Vec<(&'static str, *const ())> = Vec::new();
            flat.preorder(one, |n| got.push((n.kind_name(), n.erased_ptr())));
            assert_eq!(expected, got, "file {i}: subslice sequence must match a direct walk");
            assert_eq!(expected.len(), (hi - lo) as usize);
        }

        let mut whole = 0usize;
        flat.preorder(&pkg.syntax, |_| whole += 1);
        let mut first = 0usize;
        flat.preorder(std::slice::from_ref(&pkg.syntax[0]), |_| first += 1);
        assert!(first < whole, "subset must visit fewer nodes ({first} vs {whole})");
    }

    /// A slice that is *not* part of the built files has no events here and
    /// gets a real walk of what the caller actually handed us.
    #[test]
    fn foreign_file_slice_falls_back_to_walking() {
        let (pkg, fset) = two_file_package();
        let flat = flat_result(&pkg, &fset);

        // A file the result was never built from — its own allocation, so the
        // subslice arithmetic in `events_for` cannot mistake it for one of ours.
        let other_fset = FileSet::new();
        let outside = vec![
            parse_file(&other_fset, "a.go", SRC.as_bytes(), Mode::NONE).expect("parse outside"),
        ];
        assert!(flat.events_for(&outside).is_none());

        let mut expected = 0usize;
        preorder(NodeRef::File(&outside[0]), |_| {
            expected += 1;
            true
        });
        let mut got = 0usize;
        flat.preorder(&outside, |_| got += 1);
        assert_eq!(expected, got);
        assert!(got > 5, "expected a real walk, got {got} nodes");
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
