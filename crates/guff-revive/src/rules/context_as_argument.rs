//! `context-as-argument` — `context.Context` should be the first parameter.
//!
//! Arguments (golangci / revive):
//! ```yaml
//! - name: context-as-argument
//!   arguments:
//!     - allowTypesBefore: '*testing.T,testing.TB'
//! ```

use std::collections::HashSet;

use guff::ast::{Decl, FuncDecl};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;
use crate::settings::RuleArgument;
use crate::util::{expr_string, is_pkg_dot_type};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let allow_types = allow_types_before(pass);
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            check_func(f, &allow_types, &mut failures);
        }
    }
    failures
}

fn allow_types_before(pass: &Pass<'_>) -> HashSet<String> {
    let mut allow = HashSet::new();
    // context.Context is always allowed before another context.Context.
    allow.insert("context.Context".into());

    for arg in config::rule_arguments(pass, "context-as-argument") {
        let RuleArgument::Map(map) = arg else {
            continue;
        };
        for (key, value) in map {
            if !config::rule_option_matches(&key, "allowTypesBefore") {
                continue;
            }
            let RuleArgument::String(types) = value else {
                continue;
            };
            for ty in types.split(',') {
                let ty = ty.trim();
                if !ty.is_empty() {
                    allow.insert(ty.to_string());
                }
            }
        }
    }
    allow
}

fn check_func(f: &FuncDecl, allow_types: &HashSet<String>, failures: &mut Vec<Failure>) {
    let Some(params) = &f.ty.params else {
        return;
    };
    let params = &params.list;
    if params.len() <= 1 {
        return;
    }
    let mut ctx_allowed = true;
    for field in params {
        let is_ctx = field
            .ty
            .as_ref()
            .is_some_and(|t| is_pkg_dot_type(t, "context", "Context"));
        if is_ctx && !ctx_allowed {
            failures.push(Failure {
                rule: "context-as-argument",
                pos: field.pos().0 as u32,
                message: "context.Context should be the first parameter of a function".into(),
            });
            break;
        }
        if let Some(ty) = &field.ty {
            let rendered = expr_string(ty);
            ctx_allowed = allow_types.contains(&rendered);
        } else {
            ctx_allowed = false;
        }
    }
}
