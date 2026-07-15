//! Enabled revive rules (golint defaults when no user config is supplied).
//!
//! DEFERRED (see DEVELOPMENT.md R14): `linters.settings.revive` YAML wiring
//! (per-rule enable/disable, arguments, severity, confidence).

use std::cell::Cell;

thread_local! {
    static EXTENDED_ENABLED: Cell<bool> = const { Cell::new(false) };
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
];

/// Enables [`EXTENDED_RULES`] for the duration of `f` (integration tests).
pub fn with_extended_rules<R>(f: impl FnOnce() -> R) -> R {
    EXTENDED_ENABLED.with(|flag| flag.set(true));
    let out = f();
    EXTENDED_ENABLED.with(|flag| flag.set(false));
    out
}

/// Returns whether `name` is enabled under the current configuration.
pub fn rule_enabled(name: &str) -> bool {
    DEFAULT_RULES.contains(&name)
        || (EXTENDED_ENABLED.with(|flag| flag.get()) && EXTENDED_RULES.contains(&name))
}
