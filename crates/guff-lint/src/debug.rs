//! Verbosity of the `GUFF_DEBUG_CACHE` timing output.
//!
//! Level 1 (the variable set to anything) prints the `guff: phase …` totals that
//! have always been there; those lines are byte-identical to before this module
//! existed, so numbers recorded in `docs/PERF_TASKS*.md` stay comparable.
//! Level 2 (`GUFF_DEBUG_CACHE=2` or higher) adds the sub-phase breakdown used
//! for GO/NO-GO calls (docs/PERF_TASKS_V2.md §S-3).
//!
//! `guff-packages` has its own copy of this logic (`crates/guff-packages/src/debug.rs`)
//! and `guff-analysis` reads the variable independently — the three crates share
//! no support crate. Keep the level mapping in sync if it ever changes.

use std::sync::LazyLock;

static LEVEL: LazyLock<u8> = LazyLock::new(|| match std::env::var_os("GUFF_DEBUG_CACHE") {
    None => 0,
    // A set-but-unparsable value keeps the historical `var_os(..).is_some()`
    // behaviour of level 1 — that includes `GUFF_DEBUG_CACHE=` and `=0`, which
    // callers have always used to mean "on".
    Some(v) => v
        .to_str()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(1)
        .max(1),
});

/// Timing verbosity: 0 = off, 1 = phase totals, 2 or more = sub-phase breakdown.
fn level() -> u8 {
    *LEVEL
}

/// Whether any timing output is on (level 1 or above).
pub(crate) fn enabled() -> bool {
    level() >= 1
}

/// Whether the level-2 sub-phase breakdown is on.
pub(crate) fn detailed() -> bool {
    level() >= 2
}

/// Split the RSS the OS reports into "live" and "held by the allocator", by
/// asking mimalloc to hand back everything it can and re-reading RSS.
///
/// The per-package attribution names 1.29 GiB of a 2.16 GiB process on
/// prometheus `./...` (PERF_TASKS_V6 §4.1). That gap has two very different
/// explanations — memory nobody thought to count, or pages freed but never
/// returned — and they call for opposite work. `mi_collect(true)` decides it:
/// whatever RSS drops was the allocator's, whatever stays is live.
///
/// Debug-only (`GUFF_DEBUG_RSS`). `mi_collect` is not free and never runs
/// otherwise.
pub(crate) fn report_rss_after_collect(label: &str) {
    if !guff_packages::rss_enabled() {
        return;
    }
    let before = guff_packages::process_rss_bytes();
    // Safety: `mi_collect` is the allocator's own maintenance entry point and
    // takes no pointers; it is safe to call from any thread at any time.
    unsafe { libmimalloc_sys::mi_collect(true) };
    let after = guff_packages::process_rss_bytes();
    if std::env::var_os("GUFF_DEBUG_RSS").is_some_and(|v| v.to_str() == Some("2")) {
        // mimalloc's own ledger: `reserved` is what it took from the OS,
        // `committed`/`current` what it is holding for live blocks. If RSS is
        // far above `current`, the pages are the allocator's, not ours.
        // Safety: both are the allocator's own reporting entry points.
        unsafe {
            libmimalloc_sys::mi_stats_print(std::ptr::null_mut());
        }
    }
    if let (Some(b), Some(a)) = (before, after) {
        let mib = |v: u64| v as f64 / (1024.0 * 1024.0);
        eprintln!(
            "guff:   rss after mi_collect ({label}): {:.0} MiB → {:.0} MiB \
             (allocator gave back {:.0} MiB; {:.0} MiB is live)",
            mib(b),
            mib(a),
            mib(b.saturating_sub(a)),
            mib(a),
        );
    }
}
