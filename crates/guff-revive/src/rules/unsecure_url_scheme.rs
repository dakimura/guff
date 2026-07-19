//! `unsecure-url-scheme` — prefer https/wss over http/ws in string literals.

use guff::ast::{BasicLit, Expr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{basic_lit_string_value, is_test_package};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    if pass
        .files()
        .first()
        .is_some_and(|f| is_test_package(&f.name.name))
    {
        return Vec::new();
    }

    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::BasicLit(lit)) = n else {
                return true;
            };
            if lit.kind != Some(Token::STRING) {
                return true;
            }
            let Some(value) = basic_lit_string_value(lit) else {
                return true;
            };
            let (scheme, prefix_len) = if value.starts_with("http://") {
                ("http", 7)
            } else if value.starts_with("ws://") {
                ("ws", 5)
            } else {
                return true;
            };
            if value.len() <= prefix_len {
                return true;
            }
            if value.contains("localhost")
                || value.contains("127.0.0.1")
                || value.contains("0.0.0.0")
                || value.contains("//::")
            {
                return true;
            }
            failures.push(Failure {
                rule: "unsecure-url-scheme",
                pos: lit.value_pos.0 as u32,
                message: format!(
                    "prefer secure protocol {}s over {} in {:?}",
                    scheme,
                    scheme,
                    lit.value
                ),
            confidence: None,
        });
            true
        });
    }
    failures
}

// silence unused import
#[allow(unused_imports)]
use guff::ast::Expr as _;
