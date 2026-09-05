//! guff-revive — port of [`github.com/mgechev/revive`](https://github.com/mgechev/revive)
//! (golangci-lint wrapper in `pkg/golinters/revive`).
//!
//! Registered as golangci-lint linter name [`revive`].
//!
//! This session implements the golint-default rule subset as individual Rust
//! rules. Full revive has 80+ rules with TOML configuration.

mod astfmt;
mod config;
mod directives;
mod failure;
pub mod filefilter;
mod ifelse;
mod names;
mod revive;
mod rules;

/// Whether the enabled revive rules read `ast::Ident.obj`, the parser's
/// per-file object resolution.
///
/// Exactly one sub-check does: `defer`'s `methodCall`, which asks whether the
/// receiver of a deferred `x.M()` is a *type* (a method expression) rather than
/// a value — a question only per-file object resolution answers the way
/// upstream asks it. Type information would answer a different one: go/parser
/// resolves within a single file, so upstream is silent when the type is
/// declared in another file of the same package.
///
/// The parse-time gate calls this so the resolution walk is skipped for the
/// configurations that cannot use it — which is nearly all of them, since a
/// rule list naming `defer` with arguments usually leaves `methodCall` out.
/// Measured on tailscale (`default: none`, revive only): 5.21s with the walk
/// skipped against 5.42s with it, so this is worth asking precisely rather than
/// switching it on for every configuration that enables revive.
pub fn needs_ast_object_resolution(settings: Option<&settings::Settings>) -> bool {
    let Some(settings) = settings else {
        // No revive settings at all: the default rule set has no `defer`.
        return false;
    };
    if !settings.rule_enabled("defer", config::DEFAULT_RULES, config::all_rules()) {
        return false;
    }
    let args = settings
        .rule("defer")
        .map(|r| r.arguments.clone())
        .unwrap_or_default();
    rules::defer_allow_from_arg_list(&args).contains("methodcall")
}

mod settings;
mod util;

pub use config::{DEFAULT_RULES, EXTENDED_RULES, extended_test_settings, with_extended_rules, with_settings};
pub use revive::analyzer as revive;
pub use settings::{RuleArgument, RuleSetting, Settings};

use guff_analysis::Analyzer;

/// All analyzers in this crate.
pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![revive()]
}
