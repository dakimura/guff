//! `add-constant` — suggest named constants for magic numbers and repeated string literals.

use std::collections::{HashMap, HashSet};

use guff::ast::{BasicLit, StructType};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::basic_lit_string_value;

const STR_LIT_LIMIT: usize = 2;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    let mut struct_tags = HashSet::new();
    let mut str_counts: HashMap<String, usize> = HashMap::new();

    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::StructType(StructType { fields, .. })) = n else {
                return true;
            };
            for field in &fields.list {
                if let Some(tag) = &field.tag {
                    struct_tags.insert(tag.pos().0);
                }
            }
            true
        });
    }

    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            match n {
                Some(NodeRef::GenDecl(_)) => false, // skip declarations
                // Upstream handles a call itself and returns nil: only the
                // call's own literal arguments are checked, and the subtree is
                // not walked at all. So a literal inside a func literal passed
                // to `go f()`, or inside `f(x + 1)`, is never reported.
                Some(NodeRef::CallExpr(call)) => {
                    check_call(call, &struct_tags, &mut failures, &mut str_counts);
                    false
                }
                Some(NodeRef::BasicLit(lit)) => {
                    if !struct_tags.contains(&lit.pos().0) {
                        check_lit(lit, &mut failures, &mut str_counts);
                    }
                    true
                }
                _ => true,
            }
        });
    }
    failures
}

/// Upstream `checkFunc`: direct `BasicLit` arguments are checked, direct
/// `CallExpr` arguments recurse, and every other argument shape is ignored.
///
/// The `ignoreFuncs` option, which would skip a call by name here, is not
/// implemented — guff's add-constant reads no configuration yet.
fn check_call(
    call: &guff::ast::CallExpr,
    struct_tags: &HashSet<i64>,
    failures: &mut Vec<Failure>,
    str_counts: &mut HashMap<String, usize>,
) {
    for arg in &call.args {
        match arg {
            guff::ast::Expr::CallExpr(inner) => {
                check_call(inner, struct_tags, failures, str_counts);
            }
            guff::ast::Expr::BasicLit(lit) => {
                if !struct_tags.contains(&lit.pos().0) {
                    check_lit(lit, failures, str_counts);
                }
            }
            _ => {}
        }
    }
}

fn check_lit(lit: &BasicLit, failures: &mut Vec<Failure>, str_counts: &mut HashMap<String, usize>) {
    match lit.kind {
        Some(Token::INT) | Some(Token::FLOAT) => {
            failures.push(Failure {
                rule: "add-constant",
                pos: lit.pos().0 as u32,
                message: format!(
                    "avoid magic numbers like '{}', create a named constant for it",
                    lit.value
                ),
                ..Failure::default()
            });
        }
        Some(Token::STRING) => check_str_lit(lit, failures, str_counts),
        _ => {}
    }
}

fn check_str_lit(
    lit: &BasicLit,
    failures: &mut Vec<Failure>,
    str_counts: &mut HashMap<String, usize>,
) {
    const IGNORED: usize = usize::MAX;

    let value = lit.value.clone();
    let count = str_counts.entry(value.clone()).or_insert(0);
    if *count == IGNORED {
        return;
    }
    *count += 1;
    if basic_lit_string_value(lit).is_some_and(|s| s.is_empty()) {
        return;
    }
    if *count > STR_LIT_LIMIT {
        failures.push(Failure {
            rule: "add-constant",
            pos: lit.pos().0 as u32,
            message: format!(
                "string literal {value} appears, at least, {count} times, create a named constant for it",
                count = *count
            ),
            ..Failure::default()
        });
        *count = IGNORED;
    }
}
