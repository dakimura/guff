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
        "mirror" => Some(vec![guff_gostaticanalysis::mirror()]),
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
        "asasalint" => Some(vec![guff_style::asasalint()]),
        "bidichk" => Some(vec![guff_style::bidichk()]),
        "gochecknoinits" => Some(vec![guff_style::gochecknoinits()]),
        "gochecknoglobals" => Some(vec![guff_style::gochecknoglobals()]),
        "gocheckcompilerdirectives" => Some(vec![guff_style::gocheckcompilerdirectives()]),
        "forbidigo" => Some(vec![guff_style::forbidigo()]),
        "reassign" => Some(vec![guff_style::reassign()]),
        "recvcheck" => Some(vec![guff_style::recvcheck()]),
        "thelper" => Some(vec![guff_style::thelper()]),
        "iface" => Some(vec![guff_style::iface()]),
        "interfacebloat" => Some(vec![guff_style::interfacebloat()]),
        "inamedparam" => Some(vec![guff_style::inamedparam()]),
        "goprintffuncname" => Some(vec![guff_style::goprintffuncname()]),
        "funlen" => Some(vec![guff_style::funlen()]),
        "gocyclo" => Some(vec![guff_style::gocyclo()]),
        "lll" => Some(vec![guff_style::lll()]),
        "gocognit" => Some(vec![guff_style::gocognit()]),
        "nestif" => Some(vec![guff_style::nestif()]),
        "cyclop" => Some(vec![guff_style::cyclop()]),
        "nakedret" => Some(vec![guff_style::nakedret()]),
        "nosprintfhostport" => Some(vec![guff_style::nosprintfhostport()]),
        "predeclared" => Some(vec![guff_style::predeclared()]),
        "whitespace" => Some(vec![guff_style::whitespace()]),
        "nlreturn" => Some(vec![guff_style::nlreturn()]),
        "mnd" => Some(vec![guff_style::mnd()]),
        "prealloc" => Some(vec![guff_style::prealloc()]),
        "tagalign" => Some(vec![guff_style::tagalign()]),
        "wsl" => Some(vec![guff_style::wsl()]),
        "unconvert" => Some(vec![guff_style::unconvert()]),
        "exhaustruct" => Some(vec![guff_style::exhaustruct()]),
        "exhaustive" => Some(vec![guff_style::exhaustive()]),
        "musttag" => Some(vec![guff_style::musttag()]),
        "loggercheck" => Some(vec![guff_style::loggercheck()]),
        "sloglint" => Some(vec![guff_style::sloglint()]),
        "testifylint" => Some(vec![guff_style::testifylint()]),
        "exptostd" => Some(vec![guff_style::exptostd()]),
        "modernize" => Some(vec![guff_style::modernize()]),
        "gocritic" => Some(vec![guff_style::gocritic()]),
        "godot" => Some(vec![guff_comment::godot()]),
        "godox" => Some(vec![guff_comment::godox()]),
        "dupword" => Some(vec![guff_comment::dupword()]),
        "godoclint" => Some(vec![guff_comment::godoclint()]),
        "misspell" => Some(vec![guff_misspell::misspell()]),
        "dupl" => Some(vec![guff_dupl::dupl()]),
        "revive" => Some(vec![guff_revive::revive()]),
        "depguard" => Some(vec![guff_import::depguard()]),
        "gomoddirectives" => Some(vec![guff_import::gomoddirectives()]),
        "gomodguard" => Some(vec![guff_import::gomodguard()]),
        "importas" => Some(vec![guff_import::importas()]),
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
    "asasalint",
    "asciicheck",
    "bidichk",
    "copyloopvar",
    "cyclop",
    "depguard",
    "dogsled",
    "dupl",
    "dupword",
    "durationcheck",
    "err113",
    "errcheck",
    "errchkjson",
    "errname",
    "errorlint",
    "exhaustruct",
    "exhaustive",
    "exptostd",
    "fatcontext",
    "forbidigo",
    "forcetypeassert",
    "funlen",
    "gocheckcompilerdirectives",
    "gochecknoglobals",
    "gochecknoinits",
    "gocognit",
    "goconst",
    "gocritic",
    "gocyclo",
    "godoclint",
    "godot",
    "godox",
    "gomoddirectives",
    "gomodguard",
    "goprintffuncname",
    "govet",
    "iface",
    "importas",
    "ineffassign",
    "interfacebloat",
    "inamedparam",
    "lll",
    "loggercheck",
    "makezero",
    "mirror",
    "mnd",
    "misspell",
    "modernize",
    "musttag",
    "nakedret",
    "nestif",
    "nilnil",
    "nlreturn",
    "noctx",
    "nolintlint",
    "nosprintfhostport",
    "perfsprint",
    "prealloc",
    "predeclared",
    "reassign",
    "recvcheck",
    "revive",
    "sloglint",
    "staticcheck",
    "tagalign",
    "testifylint",
    "thelper",
    "unconvert",
    "unused",
    "usestdlibvars",
    "usetesting",
    "whitespace",
    "wrapcheck",
    "wsl",
];

/// All linter names known to the registry.
pub fn known_linter_names() -> &'static [&'static str] {
    KNOWN_LINTER_NAMES
}

/// One-line description for `guff linters` (golangci-style).
pub fn linter_description(name: &str) -> &'static str {
    match name {
        "asasalint" => "Checks for pass []any as any in variadic func(...any).",
        "asciicheck" => "Checks that identifiers do not contain non-ASCII characters.",
        "bidichk" => "Checks for dangerous unicode character sequences.",
        "copyloopvar" => "Detects unnecessary copies of loop variables (Go 1.22+).",
        "cyclop" => "Checks function and package cyclomatic complexity.",
        "depguard" => "Go linter that checks if package imports are in a list of acceptable packages.",
        "dogsled" => "Checks assignments with too many blank identifiers.",
        "dupl" => "Detects duplicate fragments of code.",
        "dupword" => "Checks for duplicate words in the source code.",
        "durationcheck" => "Checks for multiplying duration by duration.",
        "err113" => "Checks the errors handling expressions according to Go 1.13.",
        "errcheck" => "Checks for unchecked errors.",
        "errchkjson" => "Checks types passed to json encoding functions and their error handling.",
        "errname" => "Checks that sentinel errors are prefixed with Err and types with Error.",
        "errorlint" => "Finds error comparison and type assertion issues with wrapped errors.",
        "exhaustruct" => "Checks if all structure fields are initialized.",
        "exhaustive" => "Check exhaustiveness of enum switch statements.",
        "exptostd" => "Detects functions from golang.org/x/exp/ that can be replaced by std functions.",
        "fatcontext" => "Detects nested contexts in loops and function literals.",
        "forbidigo" => "Forbids identifiers",
        "forcetypeassert" => "Finds forced type assertions that may panic.",
        "funlen" => "Checks for long functions.",
        "gocheckcompilerdirectives" => "Checks that go compiler directive comments (//go:) are valid.",
        "gochecknoglobals" => "Checks that no global variables exist in Go code.",
        "gochecknoinits" => "Checks that no init functions are present in Go code.",
        "gocognit" => "Computes and checks the cognitive complexity of functions.",
        "goconst" => "Finds repeated strings that could be replaced by a constant.",
        "gocritic" => "Provides diagnostics that check for bugs, performance and style issues.",
        "gocyclo" => "Computes and checks the cyclomatic complexity of functions.",
        "godoclint" => "Checks Golang's documentation practice (godoc).",
        "godot" => "Checks that comments end in a period.",
        "godox" => "Detects FIXME, TODO and other keywords inside comments.",
        "gomoddirectives" => "Manage the use of 'replace', 'retract', and 'excludes' directives in go.mod.",
        "gomodguard" => "Allow and blocklist linter for direct Go module dependencies.",
        "goprintffuncname" => "Checks that printf-like functions are named with an f suffix.",
        "govet" => "Vet examines Go source code and reports suspicious constructs.",
        "iface" => "Detect the incorrect use of interfaces, helping developers avoid interface pollution.",
        "interfacebloat" => "A linter that checks the number of methods inside an interface.",
        "inamedparam" => "Reports interfaces with unnamed method parameters.",
        "importas" => "Enforces consistent import aliases.",
        "ineffassign" => "Detects when assignments to existing variables are not used.",
        "lll" => "Reports long lines.",
        "loggercheck" => "Checks key value pairs for common logger libraries (kitlog,klog,logr,slog,zap).",
        "makezero" => "Finds slice declarations with non-zero initial length and later appends.",
        "mirror" => "Reports wrong mirror patterns of bytes/strings usage.",
        "mnd" => "An analyzer to detect magic numbers.",
        "misspell" => "Finds commonly misspelled English words.",
        "modernize" => "Suggests simplifications to Go code using modern language and library features.",
        "musttag" => "Enforce field tags in (un)marshaled structs.",
        "nakedret" => "Checks that functions with naked returns are not longer than a maximum size.",
        "nestif" => "Reports deeply nested if statements.",
        "nilnil" => "Checks that there is no simultaneous return of nil error and an invalid value.",
        "nlreturn" => "Checks for a new line before return and branch statements to increase code clarity.",
        "noctx" => "Finds HTTP/DB/network calls that should take a context.",
        "nolintlint" => "Reports unused //nolint directives.",
        "nosprintfhostport" => "Checks for misuse of Sprintf to construct a host with port in a URL.",
        "perfsprint" => "Checks that fmt.Sprintf can be replaced with a faster alternative.",
        "prealloc" => "Finds slice declarations that could potentially be pre-allocated.",
        "predeclared" => "Finds code that shadows one of Go's predeclared identifiers.",
        "reassign" => "Checks that package variables are not reassigned.",
        "recvcheck" => "Checks for receiver type consistency.",
        "revive" => "Fast, configurable, extensible, flexible, and beautiful linter for Go. Drop-in replacement of golint.",
        "sloglint" => "Ensures consistent code style when using log/slog.",
        "staticcheck" => "Checks for bugs, performance and style issues.",
        "tagalign" => "Checks that struct tags are well aligned.",
        "testifylint" => "Checks usage of github.com/stretchr/testify.",
        "thelper" => "Detects test helpers without t.Helper() call and checks the consistency of test helpers.",
        "unconvert" => "Remove unnecessary type conversions.",
        "unused" => "Checks Go code for unused constants, variables, functions and types.",
        "usestdlibvars" => "Suggests replacing magic literals with stdlib constants.",
        "usetesting" => "Reports uses of functions with replacements in the testing package.",
        "whitespace" => "Checks for unnecessary newlines at the start and end of blocks.",
        "wrapcheck" => "Checks that errors returned from external packages are wrapped.",
        "wsl" => "Add or remove empty lines.",
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
                ..StaticcheckSettings::default()
            },
            ..LinterSettings::default()
        };
        let analyzers =
            analyzers_for_linter_with_settings("staticcheck", &settings).expect("staticcheck");
        assert!(!analyzers.iter().any(|a| a.name == "SA1004"));
        assert!(analyzers.iter().any(|a| a.name == "SA1000"));
    }
}
