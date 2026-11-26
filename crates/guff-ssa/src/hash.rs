//! Fast non-cryptographic hash maps for SSA construction.
//!
//! guff only hashes trusted local source/type keys, so SipHash's DoS resistance
//! is unnecessary. FxHash is several times faster on short keys (idents, type
//! ids, package paths). See docs/PERF_TASKS_V2.md §A-1.

pub(crate) use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
