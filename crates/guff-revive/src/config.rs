//! Enabled revive rules and per-rule arguments.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::Pass;

use crate::settings::{RuleArgument, Settings};

thread_local! {
    static THREAD_SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

/// Rule names enabled by default (mirrors revive / golangci-lint golint set).
pub const DEFAULT_RULES: &[&str] = &[
    "blank-imports",
    "context-as-argument",
    "context-keys-type",
    "dot-imports",
    "empty-block",
    "error-naming",
    "error-return",
    "error-strings",
    "errorf",
    "exported",
    "increment-decrement",
    "indent-error-flow",
    "package-comments",
    "range",
    "receiver-naming",
    "redefines-builtin-id",
    "superfluous-else",
    "time-naming",
    "unexported-return",
    "unreachable-code",
    "unused-parameter",
    "var-declaration",
    "var-naming",
];

/// Extended revive rules (off unless explicitly enabled via settings or tests).
pub const EXTENDED_RULES: &[&str] = &[
    "atomic",
    "bare-return",
    "bool-literal-in-expr",
    "call-to-gc",
    "cyclomatic",
    "duplicated-imports",
    "if-return",
    "string-of-int",
    "time-equal",
    "unchecked-type-assertion",
    "unconditional-recursion",
    "unnecessary-format",
    "use-errors-new",
    "waitgroup-by-value",
    "cognitive-complexity",
    "constant-logical-expr",
    "import-shadowing",
    "struct-tag",
    "time-date",
    "unhandled-error",
    "unnecessary-stmt",
    "add-constant",
    "argument-limit",
    "early-return",
    "deep-exit",
    "get-return",
    "redundant-import-alias",
    "unnecessary-if",
    "defer",
    "flag-parameter",
    "function-length",
    "function-result-limit",
    "use-any",
    "use-fmt-print",
    "unused-receiver",
    "modifies-parameter",
    "identical-branches",
    "identical-ifelseif-branches",
    "identical-ifelseif-conditions",
    "identical-switch-branches",
    "identical-switch-conditions",
    "line-length-limit",
    "max-control-nesting",
    "nested-structs",
    "unexported-naming",
    "empty-lines",
    "optimize-operands-order",
    "range-val-in-closure",
    "confusing-results",
    "confusing-naming",
    "imports-blocklist",
    "string-format",
    "file-header",
    "import-alias-naming",
    "useless-break",
    "useless-fallthrough",
    "modifies-value-receiver",
    "range-val-address",
    "unsecure-url-scheme",
    "banned-characters",
    "file-length-limit",
    "filename-format",
    "package-naming",
    "use-slices-sort",
    "inefficient-map-lookup",
    "redundant-test-main-exit",
    "comment-spacings",
    "epoch-naming",
    "comments-density",
    "datarace",
    "enforce-map-style",
    "enforce-slice-style",
    "enforce-switch-style",
    "enforce-repeated-arg-type-style",
    "package-directory-mismatch",
    "forbidden-call-in-wg-go",
];

/// The configured Go version (`run.go`), without cloning the whole rule list.
///
/// Version-gated rules ask this on every run, and [`effective_settings`] copies
/// every rule's arguments to answer it.
pub fn configured_go_version(pass: &Pass<'_>) -> Option<String> {
    if let Some(s) = pass.settings::<Settings>("revive") {
        return s.go.clone();
    }
    THREAD_SETTINGS.with(|slot| slot.borrow().as_ref().and_then(|s| s.go.clone()))
}

pub fn effective_settings(pass: &Pass<'_>) -> Settings {
    if let Some(s) = pass.settings::<Settings>("revive") {
        return s.clone();
    }
    THREAD_SETTINGS.with(|slot| slot.borrow().clone().unwrap_or_default())
}

/// Rules guff implements that the pinned upstream does not have yet.
///
/// golangci-lint 2.12.2 pins revive v1.15.0, and `multiline-if-init` only
/// exists on revive's master branch — v1.15.0 rejects the name outright
/// ("cannot find rule: multiline-if-init") and `enable-all-rules: true` does
/// not include it. Keeping it out of [`all_rules`] is what makes guff's
/// enable-all set the same set as upstream's; naming the rule explicitly still
/// runs it, for anyone who wants it before golangci-lint catches up.
pub const AHEAD_OF_PIN_RULES: &[&str] = &["multiline-if-init"];

/// DEFAULT ∪ EXTENDED rule names, allocated once.
///
/// This is the `enable-all-rules: true` set, so it deliberately excludes
/// [`AHEAD_OF_PIN_RULES`].
pub fn all_rules() -> &'static [&'static str] {
    static ALL: OnceLock<Vec<&'static str>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v = Vec::with_capacity(DEFAULT_RULES.len() + EXTENDED_RULES.len());
        v.extend_from_slice(DEFAULT_RULES);
        v.extend_from_slice(EXTENDED_RULES);
        v
    })
    .as_slice()
}

/// Returns whether `name` is enabled under the current configuration.
pub fn rule_enabled(pass: &Pass<'_>, name: &str) -> bool {
    effective_settings(pass).rule_enabled(name, DEFAULT_RULES, all_rules())
}

/// Severity golangci-lint puts on a revive failure.
///
/// Never empty: upstream's `normalizeConfig` defaults `revive.severity` to
/// `warning` and pushes it into every rule that does not set its own, and its
/// `severity(cfg, failure)` then answers `error` **only** for a rule whose
/// effective severity is exactly `error`. Anything else — unset, `warning`, or
/// a value revive does not know — comes out as `warning`.
///
/// Returning "" when nothing was configured (as this used to) left every
/// revive finding with an empty severity field, which no gate compared until
/// the exclusions case put a config without `revive.severity` under the golden
/// tier. The `revive` case sets it explicitly, so it never saw this.
pub fn rule_severity(pass: &Pass<'_>, name: &str) -> String {
    let settings = effective_settings(pass);
    let configured = settings.rule_severity(name).unwrap_or(SEVERITY_WARNING);
    if configured == SEVERITY_ERROR {
        SEVERITY_ERROR.to_string()
    } else {
        SEVERITY_WARNING.to_string()
    }
}

/// revive `lint.SeverityWarning` / `lint.SeverityError` (compared verbatim, so
/// `Error` is not `error`).
const SEVERITY_WARNING: &str = "warning";
const SEVERITY_ERROR: &str = "error";

pub fn rule_arguments(pass: &Pass<'_>, name: &str) -> Vec<RuleArgument> {
    effective_settings(pass)
        .rule(name)
        .map(|r| r.arguments.clone())
        .unwrap_or_default()
}

/// Normalize revive rule option names (`preserveScope` / `preserve-scope` → `preservescopes`).
pub fn normalize_rule_option(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Whether `actual` matches `expected` under revive's option spelling rules.
pub fn rule_option_matches(actual: &str, expected: &str) -> bool {
    normalize_rule_option(actual) == normalize_rule_option(expected)
}

/// True when `rule` has a string argument matching `option` (e.g. `preserveScope`).
pub fn rule_has_string_option(pass: &Pass<'_>, rule: &str, option: &str) -> bool {
    rule_arguments(pass, rule).iter().any(|arg| match arg {
        RuleArgument::String(s) => rule_option_matches(s, option),
        _ => false,
    })
}

pub fn rule_arg_string(pass: &Pass<'_>, name: &str, index: usize) -> Option<String> {
    let args = rule_arguments(pass, name);
    match args.get(index)? {
        RuleArgument::String(s) => Some(s.clone()),
        _ => None,
    }
}

pub fn rule_arg_int(pass: &Pass<'_>, name: &str, index: usize) -> Option<i64> {
    let args = rule_arguments(pass, name);
    match args.get(index)? {
        RuleArgument::Integer(n) => Some(*n),
        _ => None,
    }
}

pub fn rule_arg_string_list(pass: &Pass<'_>, name: &str, index: usize) -> Vec<String> {
    let args = rule_arguments(pass, name);
    let Some(RuleArgument::List(items)) = args.get(index) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            RuleArgument::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

pub fn rule_arg_map(pass: &Pass<'_>, name: &str, index: usize) -> Option<std::collections::HashMap<String, RuleArgument>> {
    let args = rule_arguments(pass, name);
    match args.get(index)? {
        RuleArgument::Map(m) => Some(m.clone()),
        _ => None,
    }
}

/// Every argument is a blocklisted import path, spelled flat:
/// `arguments: ["crypto/md5", "crypto/sha1"]`. Upstream's `Configure` rejects a
/// nested list outright ("Expecting a string, got []interface {}"), so reading
/// argument 0 as a list — which is what guff used to do — silently disabled the
/// rule for every config a user could actually write.
pub fn imports_blocklist_entries(pass: &Pass<'_>) -> Vec<String> {
    rule_arg_strings(pass, "imports-blocklist")
}

pub fn file_header_pattern(pass: &Pass<'_>) -> String {
    rule_arg_string(pass, "file-header", 0).unwrap_or_default()
}

/// Flat string arguments, like [`imports_blocklist_entries`].
pub fn banned_characters(pass: &Pass<'_>) -> Vec<String> {
    rule_arg_strings(pass, "banned-characters")
}

/// `arguments: [{ max: 350, skipComments: true, skipBlankLines: true }]`.
///
/// Upstream takes a k,v map here and errors on a bare int, so guff reading
/// argument 0 as an integer meant every valid config left the limit at 0 —
/// which this rule treats as "no limit", i.e. the rule was off.
pub fn file_length_limit_max(pass: &Pass<'_>) -> usize {
    let Some(map) = rule_arg_map(pass, "file-length-limit", 0) else {
        return 0;
    };
    map.iter()
        .find(|(k, _)| is_rule_option(k, "max"))
        .and_then(|(_, v)| match v {
            RuleArgument::Integer(n) => usize::try_from(*n).ok(),
            _ => None,
        })
        .unwrap_or(0)
}

/// All arguments of `name` that are strings, in order.
pub fn rule_arg_strings(pass: &Pass<'_>, name: &str) -> Vec<String> {
    rule_arguments(pass, name)
        .iter()
        .filter_map(|arg| match arg {
            RuleArgument::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// Upstream compares configuration keys case-insensitively and ignoring `-`
/// and `_`, so `max`, `Max`, `skip-comments` and `skipComments` all match
/// (`lint.isRuleOption`).
pub fn is_rule_option(key: &str, want: &str) -> bool {
    fn norm(s: &str) -> String {
        s.chars()
            .filter(|c| *c != '-' && *c != '_')
            .flat_map(char::to_lowercase)
            .collect()
    }
    norm(key) == norm(want)
}

pub fn string_format_rules(pass: &Pass<'_>) -> Vec<(String, String, String)> {
    let args = rule_arguments(pass, "string-format");
    let mut out = Vec::new();
    for arg in args {
        let RuleArgument::List(items) = arg else {
            continue;
        };
        if items.len() < 3 {
            continue;
        }
        let Some(RuleArgument::String(scope)) = items.first() else {
            continue;
        };
        let Some(RuleArgument::String(regex)) = items.get(1) else {
            continue;
        };
        let Some(RuleArgument::String(message)) = items.get(2) else {
            continue;
        };
        out.push((scope.clone(), regex.clone(), message.clone()));
    }
    out
}

fn extended_test_arguments(name: &str) -> Vec<RuleArgument> {
    match name {
        "imports-blocklist" => vec![RuleArgument::String("os".into())],
        "file-header" => vec![RuleArgument::String("Copyright".into())],
        "banned-characters" => vec![RuleArgument::String("\u{212a}".into())],
        "file-length-limit" => vec![RuleArgument::Map(HashMap::from([(
            "max".to_string(),
            RuleArgument::Integer(350),
        )]))],
        "string-format" => vec![RuleArgument::List(vec![
            RuleArgument::String("fmt.Println".into()),
            RuleArgument::String("/^ok$/".into()),
            RuleArgument::String("string must be ok".into()),
        ])],
        "comments-density" => vec![RuleArgument::Integer(10)],
        "enforce-map-style" => vec![RuleArgument::String("make".into())],
        "enforce-slice-style" => vec![RuleArgument::String("make".into())],
        _ => Vec::new(),
    }
}

/// Settings that enable golint-default + extended rules for integration tests.
pub fn extended_test_settings() -> Settings {
    let mut rules = Vec::new();
    for name in DEFAULT_RULES {
        rules.push(crate::settings::RuleSetting {
            name: (*name).to_string(),
            arguments: extended_test_arguments(name),
            disabled: false,
            severity: None,
        });
    }
    for name in EXTENDED_RULES.iter().chain(AHEAD_OF_PIN_RULES) {
        rules.push(crate::settings::RuleSetting {
            name: (*name).to_string(),
            arguments: extended_test_arguments(name),
            disabled: false,
            severity: None,
        });
    }
    Settings {
        severity: None,
        rules: Some(rules),
        // 0 rather than golangci's default 0.8, so that the rules upstream
        // reports below that threshold (modifies-parameter at 0.5,
        // optimize-operands-order at 0.3) are still exercised here. Whether a
        // default config actually surfaces them is the golden tier's business,
        // not this fixture's.
        confidence: Some(0.0),
        ignore_generated_header: false,
        enable_default_rules: false,
        enable_all_rules: false,
        go: None,
    }
}

/// Runs `f` with [`extended_test_settings`] installed (integration tests).
pub fn with_extended_rules<R>(f: impl FnOnce() -> R) -> R {
    with_settings(extended_test_settings(), f)
}

/// Runs `f` with the given revive settings (integration tests / CLI bag).
pub fn with_settings<R>(settings: Settings, f: impl FnOnce() -> R) -> R {
    THREAD_SETTINGS.with(|slot| *slot.borrow_mut() = Some(settings));
    let out = f();
    THREAD_SETTINGS.with(|slot| *slot.borrow_mut() = None);
    out
}
