//! Enabled revive rules (golint defaults when no user config is supplied).
//!
//! DEFERRED (see DEVELOPMENT.md R14): `linters.settings.revive` YAML wiring
//! (per-rule enable/disable, arguments, severity, confidence).

/// Rule names enabled by default (mirrors revive / golangci-lint golint set).
pub const DEFAULT_RULES: &[&str] = &[
    "blank-imports",
    "dot-imports",
    "empty-block",
    "error-naming",
    "error-strings",
    "increment-decrement",
    "redefines-builtin-id",
    "receiver-naming",
    "time-naming",
];

/// Returns whether `name` is enabled under the current (default) configuration.
pub fn rule_enabled(name: &str) -> bool {
    DEFAULT_RULES.contains(&name)
}
