//! Revive rule implementations (golint-default subset).

mod blank_imports;
mod context_as_argument;
mod context_keys_type;
mod dot_imports;
mod empty_block;
mod error_naming;
mod error_return;
mod error_strings;
mod errorf;
mod exported;
mod increment_decrement;
mod indent_error_flow;
mod package_comments;
mod range;
mod receiver_naming;
mod redefines_builtin_id;
mod superfluous_else;
mod time_naming;
mod unexported_return;
mod unreachable_code;
mod unused_parameter;
mod var_declaration;
mod var_naming;

use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;

pub fn run_enabled_rules(pass: &Pass<'_>) -> Vec<Failure> {
    let mut out = Vec::new();
    let mut run = |name: &str, f: fn(&Pass<'_>) -> Vec<Failure>| {
        if config::rule_enabled(name) {
            out.extend(f(pass));
        }
    };
    run("blank-imports", blank_imports::apply);
    run("context-as-argument", context_as_argument::apply);
    run("context-keys-type", context_keys_type::apply);
    run("dot-imports", dot_imports::apply);
    run("empty-block", empty_block::apply);
    run("error-naming", error_naming::apply);
    run("error-return", error_return::apply);
    run("error-strings", error_strings::apply);
    run("errorf", errorf::apply);
    run("exported", exported::apply);
    run("increment-decrement", increment_decrement::apply);
    run("indent-error-flow", indent_error_flow::apply);
    run("package-comments", package_comments::apply);
    run("range", range::apply);
    run("receiver-naming", receiver_naming::apply);
    run("redefines-builtin-id", redefines_builtin_id::apply);
    run("superfluous-else", superfluous_else::apply);
    run("time-naming", time_naming::apply);
    run("unexported-return", unexported_return::apply);
    run("unreachable-code", unreachable_code::apply);
    run("unused-parameter", unused_parameter::apply);
    run("var-declaration", var_declaration::apply);
    run("var-naming", var_naming::apply);
    out
}

// DEFERRED (R14): extended revive rules (atomic, cyclomatic, struct-tag, …)
// and `linters.settings.revive` YAML wiring.
