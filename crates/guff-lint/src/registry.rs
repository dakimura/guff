//! Analyzer registry: linter name → `go/analysis` passes.

use std::collections::HashMap;

use guff_analysis::Analyzer;

/// Names in the golangci-lint v2 `standard` preset.
pub const STANDARD_LINTER_NAMES: &[&str] = &[
    "staticcheck",
    "govet",
    "errcheck",
    "ineffassign",
    "unused",
];

/// Names in the golangci-lint v2 `fast` preset (standard minus slow linters).
pub const FAST_LINTER_NAMES: &[&str] = &["govet", "errcheck", "ineffassign", "unused"];

/// Returns analyzers registered under `name`, if any.
pub fn analyzers_for_linter(name: &str) -> Option<Vec<&'static Analyzer>> {
    match name {
        "staticcheck" => Some(guff_staticcheck::analyzers()),
        "govet" => Some(guff_govet::analyzers()),
        "errcheck" => Some(guff_errcheck::analyzers()),
        "ineffassign" => Some(guff_ineffassign::analyzers()),
        "unused" => Some(guff_unused::analyzers()),
        _ => None,
    }
}

/// All linter names known to the registry.
pub fn known_linter_names() -> &'static [&'static str] {
    STANDARD_LINTER_NAMES
}

/// Resolves a list of linter names to analyzers. Unknown names are skipped with
/// a warning via `on_unknown`.
pub fn resolve_linters(
    names: &[String],
    on_unknown: &mut dyn FnMut(&str),
) -> Vec<&'static Analyzer> {
    let mut out = Vec::new();
    let mut seen = HashMap::<&str, ()>::new();
    for name in names {
        let Some(analyzers) = analyzers_for_linter(name) else {
            on_unknown(name);
            continue;
        };
        for a in analyzers {
            if seen.insert(a.name, ()).is_none() {
                out.push(a);
            }
        }
    }
    out
}

/// Analyzers for the `standard` preset (all five linters).
pub fn standard_analyzers() -> Vec<&'static Analyzer> {
    let mut warnings = Vec::new();
    let mut on_unknown = |name: &str| warnings.push(name.to_string());
    let analyzers = resolve_linters(
        &STANDARD_LINTER_NAMES
            .iter()
            .map(|s| (*s).to_string())
            .collect::<Vec<_>>(),
        &mut on_unknown,
    );
    debug_assert!(warnings.is_empty(), "unknown standard linter: {warnings:?}");
    analyzers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_includes_staticcheck() {
        let analyzers = standard_analyzers();
        assert!(!analyzers.is_empty());
        assert!(analyzers.iter().any(|a| a.name == "SA1004"));
    }

    #[test]
    fn standard_includes_all_five_linters() {
        let analyzers = standard_analyzers();
        assert!(analyzers.iter().any(|a| a.name == "assign"));
        assert!(analyzers.iter().any(|a| a.name == "errcheck"));
        assert!(analyzers.iter().any(|a| a.name == "ineffassign"));
        assert!(analyzers.iter().any(|a| a.name == "unused"));
    }

    #[test]
    fn unknown_linter_is_skipped() {
        let mut unknown = Vec::new();
        let analyzers = resolve_linters(&["nope".into()], &mut |n| unknown.push(n.to_string()));
        assert!(analyzers.is_empty());
        assert_eq!(unknown, vec!["nope"]);
    }
}
