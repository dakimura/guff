//! Revive rule implementations (golint-default subset).

mod blank_imports;
mod dot_imports;
mod empty_block;
mod error_naming;
mod error_strings;
mod increment_decrement;
mod receiver_naming;
mod redefines_builtin_id;
mod time_naming;

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
    run("dot-imports", dot_imports::apply);
    run("empty-block", empty_block::apply);
    run("error-naming", error_naming::apply);
    run("error-strings", error_strings::apply);
    run("increment-decrement", increment_decrement::apply);
    run("redefines-builtin-id", redefines_builtin_id::apply);
    run("receiver-naming", receiver_naming::apply);
    run("time-naming", time_naming::apply);
    out
}

// DEFERRED (R14): package-comments, exported, var-naming, indent-error-flow,
// range, errorf, error-return, unexported-return, context-keys-type,
// context-as-argument, superfluous-else, unused-parameter, unreachable-code,
// var-declaration, and extended revive rules (atomic, cyclomatic, struct-tag, …).
