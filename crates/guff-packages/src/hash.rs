//! Fast non-cryptographic hash maps (A-1 extension for guff-packages).
//!
//! Same rationale as `guff-types::hash`: trusted local keys only.
pub(crate) use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
