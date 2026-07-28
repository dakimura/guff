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
