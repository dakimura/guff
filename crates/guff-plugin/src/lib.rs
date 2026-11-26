//! Module plugin API for guff — Rust counterpart of golangci-lint's
//! [`plugin-module-register`](https://github.com/golangci/plugin-module-register).
//!
//! Plugin authors register a factory with [`register!`], implement [`LinterPlugin`],
//! and link the crate into a binary built by `guff custom`.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, OnceLock};

use serde::de::DeserializeOwned;
use serde_yaml::Value;

pub use inventory;
pub use guff_analysis;
pub use guff_analysis::passes;
pub use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SettingsBag, SuggestedFix,
    TextEdit,
};

/// Error from plugin construction or analyzer build.
#[derive(Debug, Clone)]
pub struct PluginError(pub String);

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PluginError {}

impl From<String> for PluginError {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for PluginError {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Factory signature — golangci `NewPlugin` / `func(any) (LinterPlugin, error)`.
pub type NewPluginFn = fn(&Value) -> Result<Box<dyn LinterPlugin>, PluginError>;

/// A module plugin that can build `go/analysis`-style analyzers.
///
/// Equivalent to golangci's `register.LinterPlugin`.
pub trait LinterPlugin: Send + Sync {
    /// Build analyzers for this plugin instance (after settings were applied in `New`).
    fn build_analyzers(&self) -> Result<Vec<&'static Analyzer>, PluginError>;

    /// Optional one-line description for `guff linters` (config may override).
    fn description(&self) -> &'static str {
        ""
    }
}

/// Inventory entry submitted by [`register!`].
pub struct PluginRegistration {
    pub name: &'static str,
    pub new: NewPluginFn,
}

inventory::collect!(PluginRegistration);

/// Register a plugin factory under `name` (golangci `register.Plugin`).
///
/// Place at crate root so linking the plugin crate registers it:
///
/// ```ignore
/// guff_plugin::register!("example", new_example);
/// ```
#[macro_export]
macro_rules! register {
    ($name:expr, $new:expr) => {
        $crate::inventory::submit! {
            $crate::PluginRegistration {
                name: $name,
                new: $new,
            }
        }
    };
}

/// Decode YAML settings into `T` (golangci `register.DecodeSettings`).
pub fn decode_settings<T: DeserializeOwned>(settings: &Value) -> Result<T, PluginError> {
    if settings.is_null() {
        return serde_yaml::from_value(Value::Mapping(serde_yaml::Mapping::new()))
            .map_err(|e| PluginError(format!("decode settings: {e}")));
    }
    serde_yaml::from_value(settings.clone())
        .map_err(|e| PluginError(format!("decode settings: {e}")))
}

fn manual_factories() -> &'static Mutex<HashMap<&'static str, NewPluginFn>> {
    static MANUAL: OnceLock<Mutex<HashMap<&'static str, NewPluginFn>>> = OnceLock::new();
    MANUAL.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Manually register a factory (tests, or when not using [`register!`] / inventory).
pub fn register_factory(name: &'static str, new: NewPluginFn) {
    if let Ok(mut guard) = manual_factories().lock() {
        guard.insert(name, new);
    }
}

/// Clear manual factories (tests).
pub fn clear_manual_factories() {
    if let Ok(mut guard) = manual_factories().lock() {
        guard.clear();
    }
}

/// Look up a registered factory by linter name.
pub fn factory_for(name: &str) -> Option<NewPluginFn> {
    if let Ok(guard) = manual_factories().lock() {
        if let Some(&f) = guard.get(name) {
            return Some(f);
        }
    }
    inventory::iter::<PluginRegistration>
        .into_iter()
        .find(|r| r.name == name)
        .map(|r| r.new)
}

/// True if a plugin factory named `name` was linked into this binary.
pub fn is_registered(name: &str) -> bool {
    factory_for(name).is_some()
}

/// Names of all linked / manually registered plugin factories.
pub fn registered_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = inventory::iter::<PluginRegistration>
        .into_iter()
        .map(|r| r.name)
        .collect();
    if let Ok(guard) = manual_factories().lock() {
        names.extend(guard.keys().copied());
    }
    names.sort_unstable();
    names.dedup();
    names
}

struct Instantiated {
    analyzers: Vec<&'static Analyzer>,
    description: String,
}

fn instance_cache() -> &'static Mutex<HashMap<String, Instantiated>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Instantiated>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Clear instantiated plugins (tests).
pub fn clear_instances() {
    if let Ok(mut guard) = instance_cache().lock() {
        guard.clear();
    }
}

/// Instantiate (or reuse) a plugin by name with the given YAML settings.
///
/// Mirrors golangci calling `New(settings)` then `BuildAnalyzers()`.
/// `description_override` is used when the plugin returns an empty description
/// (typically from `linters.settings.custom.<name>.description`).
pub fn instantiate(
    name: &str,
    settings: &Value,
) -> Result<Vec<&'static Analyzer>, PluginError> {
    instantiate_with_description(name, settings, "")
}

/// Like [`instantiate`], with an optional description from config.
pub fn instantiate_with_description(
    name: &str,
    settings: &Value,
    description_override: &str,
) -> Result<Vec<&'static Analyzer>, PluginError> {
    {
        let guard = instance_cache()
            .lock()
            .map_err(|_| PluginError("plugin instance cache poisoned".into()))?;
        if let Some(inst) = guard.get(name) {
            return Ok(inst.analyzers.clone());
        }
    }

    let new_fn = factory_for(name).ok_or_else(|| {
        PluginError(format!("plugin {name:?} is not registered in this binary"))
    })?;
    let plugin = new_fn(settings)?;
    let mut description = plugin.description().to_string();
    if description.is_empty() && !description_override.is_empty() {
        description = description_override.to_string();
    }
    let analyzers = plugin.build_analyzers()?;

    let mut guard = instance_cache()
        .lock()
        .map_err(|_| PluginError("plugin instance cache poisoned".into()))?;
    guard.insert(
        name.to_string(),
        Instantiated {
            analyzers: analyzers.clone(),
            description,
        },
    );
    Ok(analyzers)
}

/// Description from a previously instantiated plugin, if any.
pub fn instantiated_description(name: &str) -> Option<String> {
    instance_cache()
        .lock()
        .ok()
        .and_then(|g| g.get(name).map(|i| i.description.clone()))
}

/// Analyzers from a previously instantiated plugin, if any.
pub fn instantiated_analyzers(name: &str) -> Option<Vec<&'static Analyzer>> {
    instance_cache()
        .lock()
        .ok()
        .and_then(|g| g.get(name).map(|i| i.analyzers.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

    struct DummyPlugin;

    impl LinterPlugin for DummyPlugin {
        fn build_analyzers(&self) -> Result<Vec<&'static Analyzer>, PluginError> {
            Ok(vec![dummy_analyzer()])
        }

        fn description(&self) -> &'static str {
            "dummy plugin for tests"
        }
    }

    fn new_dummy(_settings: &Value) -> Result<Box<dyn LinterPlugin>, PluginError> {
        Ok(Box::new(DummyPlugin))
    }

    fn dummy_run(_pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
        Ok(None)
    }

    fn dummy_analyzer() -> &'static Analyzer {
        static A: OnceLock<Analyzer> = OnceLock::new();
        A.get_or_init(|| Analyzer {
            name: "dummy_plugin_analyzer",
            doc: "test",
            url: "",
            run: dummy_run as RunFn,
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        })
    }

    #[test]
    fn decode_settings_empty_object() {
        #[derive(Debug, serde::Deserialize, PartialEq)]
        struct S {
            #[serde(default)]
            message: String,
        }
        let s = decode_settings::<S>(&Value::Null).unwrap();
        assert_eq!(s.message, "");
    }

    #[test]
    fn instantiate_via_manual_factory() {
        clear_instances();
        clear_manual_factories();
        register_factory("dummy_test_plugin", new_dummy);
        let analyzers = instantiate("dummy_test_plugin", &Value::Null).unwrap();
        assert_eq!(analyzers.len(), 1);
        assert_eq!(analyzers[0].name, "dummy_plugin_analyzer");
        assert_eq!(
            instantiated_description("dummy_test_plugin").as_deref(),
            Some("dummy plugin for tests")
        );
        clear_instances();
        clear_manual_factories();
    }
}
