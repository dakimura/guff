//! Per-analyzer settings bag passed through [`Pass`](crate::Pass).
//!
//! Settings are keyed by linter / analyzer name (e.g. `"errcheck"`) and stored
//! as type-erased values. Analyzers downcast with [`SettingsBag::get`].

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// Type-erased settings shared by all analyzers in one runner invocation.
#[derive(Default, Clone)]
pub struct SettingsBag {
    map: HashMap<String, Arc<dyn Any + Send + Sync>>,
}

impl SettingsBag {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a typed settings value under `key` (typically a linter name).
    pub fn insert<T: Any + Send + Sync>(&mut self, key: impl Into<String>, value: T) {
        self.map.insert(key.into(), Arc::new(value));
    }

    /// Borrow a typed settings value previously inserted under `key`.
    pub fn get<T: Any + Send + Sync>(&self, key: &str) -> Option<&T> {
        self.map.get(key).and_then(|v| v.downcast_ref::<T>())
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }
}

impl fmt::Debug for SettingsBag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Keys are sorted so the output is deterministic across runs. This
        // matters beyond debugging: the issues-cache salt fingerprints the
        // settings via this `Debug` impl, and an unsorted `HashMap` iteration
        // order would change the salt every run, flipping every package between
        // cache hit and miss (see guff-runner cache salt).
        let mut keys: Vec<&String> = self.map.keys().collect();
        keys.sort();
        f.debug_struct("SettingsBag")
            .field("keys", &keys)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Sample {
        n: i32,
    }

    #[test]
    fn insert_and_get_roundtrip() {
        let mut bag = SettingsBag::new();
        bag.insert("errcheck", Sample { n: 7 });
        assert_eq!(bag.get::<Sample>("errcheck"), Some(&Sample { n: 7 }));
        assert!(bag.get::<Sample>("govet").is_none());
        assert!(bag.get::<i32>("errcheck").is_none());
    }
}
