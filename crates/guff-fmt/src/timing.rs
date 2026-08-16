//! Where the `format_checks` phase actually spends its time (PERF_TASKS_V8 §V8-2).
//!
//! The phase was reported as one number — `format multi 0.63s` — for its whole
//! life, because it overlapped analysis and nobody had a reason to open it. This
//! splits that number into the stages a fix would have to target: reading the
//! file, the shared gci+gofumpt parse, each formatter's own `format`, and the
//! diff that turns "output differs" into a line number.
//!
//! Cost discipline is the same as PERF_TASKS_V2 B-0 and
//! `passes::inspect::PreorderCounters`: the env flag is read once into a
//! `LazyLock<bool>`, the clock is read only when it is on, and the counters are
//! per-thread `Arc`s parked in a registry so the hot path does uncontended
//! relaxed adds and the reporter can still sum across rayon workers.
//!
//! `guff-fmt` does not depend on `guff-lint`, so the `GUFF_DEBUG_CACHE` level
//! mapping is duplicated here — the same duplication `guff-packages` and
//! `guff-analysis` already carry. Keep the three in sync.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

/// A stage of the per-file check, in the order the file passes through them.
#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum Stage {
    /// `fs::read` plus the exclude / generated / build-tag pre-filters.
    Read = 0,
    /// The one parse native gci and gofumpt share (B-10).
    SharedParse = 1,
    /// gci's own work on top of that parse: assign sections, reconstruct the
    /// import block, and gofmt the result.
    Gci = 2,
    /// gofumpt's rules and printer, running on the shared AST.
    Gofumpt = 3,
    /// A formatter's own `format` call — everything not on the shared path.
    /// On the default `.golangci.yml` set that is goimports, which parses
    /// independently because it needs a different parser mode.
    Format = 4,
    /// `TextDiff::from_lines` + `first_changed_lines`, only for files that
    /// actually changed.
    ///
    /// The shared path runs its own copy of that diff, and it is *not* counted
    /// here — it is already inside [`Stage::Gci`] / [`Stage::Gofumpt`], and
    /// nesting the two would make the percentages sum past 100. On a formatted
    /// tree it is dead weight either way: the output equals the source, so no
    /// diff runs.
    Diff = 5,
}

impl Stage {
    const COUNT: usize = 6;

    /// No stage is timed inside another, so these sum to the phase's CPU.
    const NAMES: [&'static str; Self::COUNT] = [
        "read+filter",
        "shared parse",
        "gci",
        "gofumpt",
        "format (own parse)",
        "diff",
    ];
}

/// Nanoseconds and call counts per [`Stage`], for one thread.
#[derive(Default)]
struct Counters {
    nanos: [AtomicU64; Stage::COUNT],
    calls: [AtomicU64; Stage::COUNT],
    /// Bytes handed to `fs::read`, so throughput is a division and not a guess.
    bytes: AtomicU64,
    /// Files whose formatters were all served from the warm format cache.
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    /// Files where gci's reconstructed source came back byte-identical and the
    /// closing gofmt could reuse the shared parse instead of doing its own.
    gci_parse_reused: AtomicU64,
    gci_parse_redone: AtomicU64,
}

static ENABLED: LazyLock<bool> = LazyLock::new(|| {
    // Level 2 and up, matching `guff_lint::debug::detailed`. A set-but-
    // unparsable value means level 1 there, so it means "off" here.
    std::env::var("GUFF_DEBUG_CACHE")
        .ok()
        .and_then(|v| v.trim().parse::<u8>().ok())
        .is_some_and(|n| n >= 2)
});

static REGISTRY: LazyLock<Mutex<Vec<Arc<Counters>>>> = LazyLock::new(|| Mutex::new(Vec::new()));

thread_local! {
    static LOCAL: Arc<Counters> = {
        let c = Arc::new(Counters::default());
        REGISTRY.lock().unwrap_or_else(|e| e.into_inner()).push(Arc::clone(&c));
        c
    };
}

/// Whether the stage breakdown is being collected (`GUFF_DEBUG_CACHE` ≥ 2).
#[inline]
pub(crate) fn enabled() -> bool {
    *ENABLED
}

/// Times `f` against `stage` when accounting is on, and is `f()` when it is not.
///
/// The `Instant::now()` pair is taken per *file per stage*, never per line or
/// per node — on prometheus that is ~2,900 clock reads for a 0.6s phase.
#[inline]
pub(crate) fn timed<T>(stage: Stage, f: impl FnOnce() -> T) -> T {
    if !*ENABLED {
        return f();
    }
    let start = Instant::now();
    let out = f();
    let nanos = start.elapsed().as_nanos() as u64;
    LOCAL.with(|c| {
        c.nanos[stage as usize].fetch_add(nanos, Ordering::Relaxed);
        c.calls[stage as usize].fetch_add(1, Ordering::Relaxed);
    });
    out
}

/// Records `n` bytes read from disk.
#[inline]
pub(crate) fn add_bytes(n: u64) {
    if !*ENABLED {
        return;
    }
    LOCAL.with(|c| c.bytes.fetch_add(n, Ordering::Relaxed));
}

/// Records one file as fully served from the format cache, or not.
#[inline]
pub(crate) fn add_cache(hit: bool) {
    if !*ENABLED {
        return;
    }
    LOCAL.with(|c| {
        if hit {
            c.cache_hits.fetch_add(1, Ordering::Relaxed)
        } else {
            c.cache_misses.fetch_add(1, Ordering::Relaxed)
        }
    });
}

/// Records whether gci's closing gofmt reused the shared parse.
#[inline]
pub(crate) fn add_gci_parse(reused: bool) {
    if !*ENABLED {
        return;
    }
    LOCAL.with(|c| {
        if reused {
            c.gci_parse_reused.fetch_add(1, Ordering::Relaxed)
        } else {
            c.gci_parse_redone.fetch_add(1, Ordering::Relaxed)
        }
    });
}

/// One line per stage plus a totals line, or an empty `Vec` when accounting is
/// off. Rendering lives here so the caller does not have to know the stage list.
pub fn report() -> Vec<String> {
    if !*ENABLED {
        return Vec::new();
    }
    let reg = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    let mut nanos = [0u64; Stage::COUNT];
    let mut calls = [0u64; Stage::COUNT];
    let (mut bytes, mut hits, mut misses) = (0u64, 0u64, 0u64);
    let (mut reused, mut redone) = (0u64, 0u64);
    for c in reg.iter() {
        for i in 0..Stage::COUNT {
            nanos[i] += c.nanos[i].load(Ordering::Relaxed);
            calls[i] += c.calls[i].load(Ordering::Relaxed);
        }
        bytes += c.bytes.load(Ordering::Relaxed);
        hits += c.cache_hits.load(Ordering::Relaxed);
        misses += c.cache_misses.load(Ordering::Relaxed);
        reused += c.gci_parse_reused.load(Ordering::Relaxed);
        redone += c.gci_parse_redone.load(Ordering::Relaxed);
    }
    let total: u64 = nanos.iter().sum();
    if total == 0 {
        return Vec::new();
    }
    // These are worker-thread sums, not wall — the same caveat the
    // `per-analyzer analyze time` table carries (PERF_TASKS §1.6). Saying so in
    // the output is cheaper than a reader mistaking 1.9s of CPU for 1.9s of
    // wall on a phase whose whole point is that it overlaps.
    let mut out = vec![format!(
        "guff:     format stage CPU (summed over {} fmt threads, not wall):",
        reg.len().max(1),
    )];
    for i in 0..Stage::COUNT {
        out.push(format!(
            "  {:>22} {:>7.3}s over {:>6} calls ({:.1}%)",
            Stage::NAMES[i],
            nanos[i] as f64 / 1e9,
            calls[i],
            nanos[i] as f64 / total as f64 * 100.0,
        ));
    }
    out.push(format!(
        "  {:>22} {:>7.3}s, {:.1} MiB read, cache {hits} hit / {misses} miss",
        "total",
        total as f64 / 1e9,
        bytes as f64 / (1024.0 * 1024.0),
    ));
    if reused + redone > 0 {
        out.push(format!(
            "  {:>22} {reused} reused the shared parse, {redone} reparsed",
            "gci gofmt",
        ));
    }
    out
}
