//! Fast non-cryptographic hash maps for the analysis driver.
//!
//! guff only hashes trusted local paths/analyzer names, so SipHash's DoS
//! resistance is unnecessary. See docs/PERF_TASKS_V2.md §A-1.

pub(crate) use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
