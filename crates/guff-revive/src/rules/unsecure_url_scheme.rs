//! `unsecure-url-scheme` — prefer https/wss over http/ws in string literals.

use guff::ast::BasicLit;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{basic_lit_string_value, is_test_package};

pub struct Checker {
    failures: Vec<Failure>,
}

impl Checker {
    pub fn try_new(pass: &Pass<'_>) -> Option<Self> {
        if pass
            .files()
            .first()
            .is_some_and(|f| is_test_package(&f.name.name))
        {
            return None;
        }
        Some(Self {
            failures: Vec::new(),
        })
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::BasicLit(lit) = n else {
            return;
        };
        if lit.kind != Some(Token::STRING) {
            return;
        }
        let Some(value) = basic_lit_string_value(lit) else {
            return;
        };
        let (scheme, prefix_len) = if value.starts_with("http://") {
            ("http", 7)
        } else if value.starts_with("ws://") {
            ("ws", 5)
        } else {
            return;
        };
        if value.len() <= prefix_len {
            return;
        }
        if value.contains("localhost")
            || value.contains("127.0.0.1")
            || value.contains("0.0.0.0")
            || value.contains("//::")
        {
            return;
        }
        self.failures.push(Failure {
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
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let Some(mut c) = Checker::try_new(pass) else {
        return Vec::new();
    };
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}
