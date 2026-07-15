//! Enabled revive rules and per-rule arguments.

use std::cell::RefCell;

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
    "multiline-if-init",
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

pub fn effective_settings(pass: &Pass<'_>) -> Settings {
    if let Some(s) = pass.settings::<Settings>("revive") {
        return s.clone();
    }
    THREAD_SETTINGS.with(|slot| slot.borrow().clone().unwrap_or_default())
}

/// Returns whether `name` is enabled under the current configuration.
pub fn rule_enabled(pass: &Pass<'_>, name: &str) -> bool {
    let settings = effective_settings(pass);
    if let Some(rules) = settings.rules.as_ref() {
        return rules.iter().any(|r| r.name == name && !r.disabled);
    }
    DEFAULT_RULES.contains(&name)
}

pub fn rule_severity(pass: &Pass<'_>, name: &str) -> String {
    effective_settings(pass)
        .rule_severity(name)
        .unwrap_or_default()
        .to_string()
}

pub fn rule_arguments(pass: &Pass<'_>, name: &str) -> Vec<RuleArgument> {
    effective_settings(pass)
        .rule(name)
        .map(|r| r.arguments.clone())
        .unwrap_or_default()
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

pub fn imports_blocklist_entries(pass: &Pass<'_>) -> Vec<String> {
    rule_arg_string_list(pass, "imports-blocklist", 0)
}

pub fn file_header_pattern(pass: &Pass<'_>) -> String {
    rule_arg_string(pass, "file-header", 0).unwrap_or_default()
}

pub fn banned_characters(pass: &Pass<'_>) -> Vec<String> {
    rule_arg_string_list(pass, "banned-characters", 0)
}

pub fn file_length_limit_max(pass: &Pass<'_>) -> usize {
    rule_arg_int(pass, "file-length-limit", 0)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(0)
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
        "imports-blocklist" => vec![RuleArgument::List(vec![RuleArgument::String("os".into())])],
        "file-header" => vec![RuleArgument::String("Copyright".into())],
        "banned-characters" => vec![RuleArgument::List(vec![RuleArgument::String("\u{212a}".into())])],
        "file-length-limit" => vec![RuleArgument::Integer(350)],
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
    for name in EXTENDED_RULES {
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
        confidence: None,
        ignore_generated_header: false,
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
