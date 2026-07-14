//! Analyzer registry: linter name → `go/analysis` passes.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::Analyzer;

use crate::settings::LinterSettings;

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
    analyzers_for_linter_with_settings(name, &LinterSettings::default())
}

/// Like [`analyzers_for_linter`], applying `linters.settings` (govet/staticcheck filters).
pub fn analyzers_for_linter_with_settings(
    name: &str,
    settings: &LinterSettings,
) -> Option<Vec<&'static Analyzer>> {
    let analyzers = match name {
        "staticcheck" => Some(guff_staticcheck::analyzers()),
        "govet" => Some(guff_govet::analyzers()),
        "errcheck" => Some(guff_errcheck::analyzers()),
        "ineffassign" => Some(guff_ineffassign::analyzers()),
        "unused" => Some(guff_unused::analyzers()),
        "forcetypeassert" => Some(vec![guff_gostaticanalysis::forcetypeassert()]),
        "nilnil" => Some(vec![guff_gostaticanalysis::nilnil()]),
        "makezero" => Some(vec![guff_gostaticanalysis::makezero()]),
        "errname" => Some(vec![guff_error::errname()]),
        "err113" => Some(vec![guff_error::err113()]),
        "durationcheck" => Some(vec![guff_error::durationcheck()]),
        "errorlint" => Some(vec![guff_error::errorlint()]),
        "wrapcheck" => Some(vec![guff_error::wrapcheck()]),
        "errchkjson" => Some(vec![guff_error::errchkjson()]),
        "noctx" => Some(vec![guff_context::noctx()]),
        "fatcontext" => Some(vec![guff_context::fatcontext()]),
        "copyloopvar" => Some(vec![guff_style::copyloopvar()]),
        "usetesting" => Some(vec![guff_style::usetesting()]),
        "usestdlibvars" => Some(vec![guff_style::usestdlibvars()]),
        "perfsprint" => Some(vec![guff_style::perfsprint()]),
        "goconst" => Some(vec![guff_style::goconst()]),
        "dogsled" => Some(vec![guff_style::dogsled()]),
        "asciicheck" => Some(vec![guff_style::asciicheck()]),
        "goprintffuncname" => Some(vec![guff_style::goprintffuncname()]),
        "funlen" => Some(vec![guff_style::funlen()]),
        "gocyclo" => Some(vec![guff_style::gocyclo()]),
        "lll" => Some(vec![guff_style::lll()]),
        // Meta / post-processor linters (no go/analysis passes).
        "nolintlint" => Some(Vec::new()),
        _ => None,
    }?;
    Some(settings.apply_to_analyzers(name, analyzers))
}

/// True for linters implemented as post-processors (no Analyzer DAG nodes).
pub fn is_meta_linter(name: &str) -> bool {
    matches!(name, "nolintlint")
}

/// All linter names known to the registry (including meta / post-processor ones).
pub const KNOWN_LINTER_NAMES: &[&str] = &[
    "asciicheck",
    "copyloopvar",
    "dogsled",
    "durationcheck",
    "err113",
    "errcheck",
    "errchkjson",
    "errname",
    "errorlint",
    "fatcontext",
    "forcetypeassert",
    "funlen",
    "goconst",
    "gocyclo",
    "goprintffuncname",
    "govet",
    "ineffassign",
    "lll",
    "makezero",
    "nilnil",
    "noctx",
    "nolintlint",
    "perfsprint",
    "staticcheck",
    "unused",
    "usestdlibvars",
    "usetesting",
    "wrapcheck",
];

/// All linter names known to the registry.
pub fn known_linter_names() -> &'static [&'static str] {
    KNOWN_LINTER_NAMES
}

/// One-line description for `guff linters` (golangci-style).
pub fn linter_description(name: &str) -> &'static str {
    match name {
        "asciicheck" => "Checks that identifiers do not contain non-ASCII characters.",
        "copyloopvar" => "Detects unnecessary copies of loop variables (Go 1.22+).",
        "dogsled" => "Checks assignments with too many blank identifiers.",
        "durationcheck" => "Checks for multiplying duration by duration.",
        "err113" => "Checks the errors handling expressions according to Go 1.13.",
        "errcheck" => "Checks for unchecked errors.",
        "errchkjson" => "Checks types passed to json encoding functions and their error handling.",
        "errname" => "Checks that sentinel errors are prefixed with Err and types with Error.",
        "errorlint" => "Finds error comparison and type assertion issues with wrapped errors.",
        "fatcontext" => "Detects nested contexts in loops and function literals.",
        "forcetypeassert" => "Finds forced type assertions that may panic.",
        "funlen" => "Checks for long functions.",
        "goconst" => "Finds repeated strings that could be replaced by a constant.",
        "gocyclo" => "Computes and checks the cyclomatic complexity of functions.",
        "goprintffuncname" => "Checks that printf-like functions are named with an f suffix.",
        "govet" => "Vet examines Go source code and reports suspicious constructs.",
        "ineffassign" => "Detects when assignments to existing variables are not used.",
        "lll" => "Reports long lines.",
        "makezero" => "Finds slice declarations with non-zero initial length and later appends.",
        "nilnil" => "Checks that there is no simultaneous return of nil error and an invalid value.",
        "noctx" => "Finds HTTP/DB/network calls that should take a context.",
        "nolintlint" => "Reports unused //nolint directives.",
        "perfsprint" => "Checks that fmt.Sprintf can be replaced with a faster alternative.",
        "staticcheck" => "Checks for bugs, performance and style issues.",
        "unused" => "Checks Go code for unused constants, variables, functions and types.",
        "usestdlibvars" => "Suggests replacing magic literals with stdlib constants.",
        "usetesting" => "Reports uses of functions with replacements in the testing package.",
        "wrapcheck" => "Checks that errors returned from external packages are wrapped.",
        _ => "",
    }
}

/// Split known linters into enabled / disabled sets for the current selection.
///
/// Unknown names in the selection (not yet implemented) are listed under enabled
/// so `guff linters` still reflects the config; they may not run.
pub fn partition_linters(selection: &crate::config::LinterSelection) -> (Vec<String>, Vec<String>) {
    let mut enabled = selection.resolve_names();
    enabled.sort();
    let enabled_set: std::collections::HashSet<String> = enabled.iter().cloned().collect();

    let mut disabled: Vec<String> = known_linter_names()
        .iter()
        .copied()
        .filter(|n| !enabled_set.contains(*n))
        .map(|s| s.to_string())
        .collect();
    disabled.sort();

    (enabled, disabled)
}

/// Format golangci-lint–style enabled/disabled listing to `out`.
pub fn format_linters_listing(
    enabled: &[String],
    disabled: &[String],
    out: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    writeln!(out, "Enabled by your configuration linters:")?;
    for name in enabled {
        let desc = linter_description(name);
        if desc.is_empty() {
            writeln!(out, "{name}")?;
        } else {
            writeln!(out, "{name}: {desc}")?;
        }
    }
    writeln!(out)?;
    writeln!(out, "Disabled by your configuration linters:")?;
    for name in disabled {
        let desc = linter_description(name);
        if desc.is_empty() {
            writeln!(out, "{name}")?;
        } else {
            writeln!(out, "{name}: {desc}")?;
        }
    }
    Ok(())
}

/// Resolves a list of linter names to analyzers. Unknown names are skipped with
/// a warning via `on_unknown`.
pub fn resolve_linters(
    names: &[String],
    on_unknown: &mut dyn FnMut(&str),
) -> Vec<&'static Analyzer> {
    resolve_linters_with_settings(names, &LinterSettings::default(), on_unknown)
}

/// Like [`resolve_linters`], applying per-linter settings filters.
pub fn resolve_linters_with_settings(
    names: &[String],
    settings: &LinterSettings,
    on_unknown: &mut dyn FnMut(&str),
) -> Vec<&'static Analyzer> {
    let mut out = Vec::new();
    let mut seen = HashMap::<&str, ()>::new();
    for name in names {
        let Some(analyzers) = analyzers_for_linter_with_settings(name, settings) else {
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

/// Map an analyzer pass name (`errcheck`, `SA1004`, `printf`, …) to its
/// golangci linter name (`errcheck`, `staticcheck`, `govet`, …).
///
/// Unknown analyzers are returned unchanged (so exclude-rules that name a
/// pass directly still work).
pub fn linter_name_for_analyzer(analyzer: &str) -> &str {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for &linter in KNOWN_LINTER_NAMES {
            if is_meta_linter(linter) {
                continue;
            }
            if let Some(analyzers) = analyzers_for_linter(linter) {
                for a in analyzers {
                    m.insert(a.name, linter);
                }
            }
        }
        m
    });
    map.get(analyzer).copied().unwrap_or(analyzer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{GovetSettings, LinterSettings, StaticcheckSettings};

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

    #[test]
    fn govet_settings_disable_all_enable_printf() {
        let settings = LinterSettings {
            govet: GovetSettings {
                disable_all: true,
                enable: vec!["printf".into()],
                ..GovetSettings::default()
            },
            ..LinterSettings::default()
        };
        let analyzers =
            analyzers_for_linter_with_settings("govet", &settings).expect("govet");
        assert_eq!(analyzers.len(), 1);
        assert_eq!(analyzers[0].name, "printf");
    }

    #[test]
    fn staticcheck_settings_disable_check() {
        let settings = LinterSettings {
            staticcheck: StaticcheckSettings {
                checks: Some(vec!["all".into(), "-SA1004".into()]),
            },
            ..LinterSettings::default()
        };
        let analyzers =
            analyzers_for_linter_with_settings("staticcheck", &settings).expect("staticcheck");
        assert!(!analyzers.iter().any(|a| a.name == "SA1004"));
        assert!(analyzers.iter().any(|a| a.name == "SA1000"));
    }
}
